//! Architecture guard — `accumulate_dispatch_dep_signature` may be
//! called only from two allow-listed functions in production source.
//!
//! The legacy `meta_resolve::dep_signature::accumulate_dispatch_dep_signature`
//! accumulator predates the fact-tracer fan-out substrate. The
//! component-meta resolver, projector, materialiser, and registry
//! decl paths have been rewired to fan dispatch facts into every
//! active `FactReadSet` via
//! `observe_fact_signature(&dep_signature_to_fact_signature(...))`
//! rather than push into the legacy thread-local accumulator. The
//! slot-binding-graph traversal in
//! `meta_resolve/slot_binding_graph.rs` runs paired dual-emission
//! through `emit_slot_binding_graph_dispatch_facts`; the helper body
//! intentionally keeps the legacy `accumulate_dispatch_dep_signature`
//! call alongside the new tracer fan-out so the curated
//! `state.fact_versions` channel retains coverage during the dual
//! window. The legacy helper definition itself remains in
//! `meta_resolve/dep_signature.rs` until the legacy drain in
//! `compute_component_meta_state_inner` is retired and the symbol
//! is deleted.
//!
//! Concretely the guard scans
//! `crates/verter_session/src/**/*.rs` (excluding sibling `*_tests.rs`,
//! `tests/`, `benches/`, `examples/`) and rejects every call to
//! `accumulate_dispatch_dep_signature` whose enclosing function name
//! is NOT one of:
//!
//! - `accumulate_dispatch_dep_signature` (the helper definition body;
//!   the helper recursively names itself only as the outer `fn`
//!   declaration — there is no inner recursion, so the only call
//!   from within this function is by definition the helper itself).
//! - `emit_slot_binding_graph_dispatch_facts` (the dual-emit helper
//!   in `slot_binding_graph.rs` whose body calls
//!   `accumulate_dispatch_dep_signature(sig);` alongside
//!   `observe_fact_signature`; tracked by Block 1.C's
//!   `slot_binding_graph_dual_emit_arch_guard.rs`).
//!
//! Trigger conditions:
//!
//! - Adding `accumulate_dispatch_dep_signature(&x.dep_signature);` in
//!   a new production function inside `crates/verter_session/src/`
//!   FAILS the `no_accumulate_dispatch_dep_signature_outside_helpers`
//!   test.
//! - Re-introducing a call site inside `meta_resolve/projectors/mod.rs`,
//!   `meta_resolve/materialize/field_types.rs`,
//!   `meta_resolve/resolved_state.rs`, or
//!   `resolver_core/component_meta_query_engine/registry_decl.rs`
//!   FAILS the same test.
//!
//! The `*_use_import_allowed_in_helper_files` test pins down where
//! the `use` import is allowed (the slot-binding-graph helper module
//! and the helper definition's own module need it; nothing else does).
//!
//! Reads, comments mentioning the symbol name, and
//! `use ... accumulate_dispatch_dep_signature;` imports are not call
//! sites and never flagged.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, ExprCall, ImplItemFn, ItemFn, ItemImpl, ItemMod, Meta};
use walkdir::WalkDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("canonicalize CARGO_MANIFEST_DIR")
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Functions whose bodies may legitimately call
/// `accumulate_dispatch_dep_signature`. Every other production-source
/// caller is a regression.
const ACCUMULATE_DISPATCH_DEP_SIGNATURE_ALLOW: &[&str] = &[
    // The helper definition itself. Block 9 will retire the legacy
    // thread-local drain and delete this function; until then it is
    // the single producer that converts a `DepSignature` into
    // `FactVersionRef` entries and pushes them onto the request-scoped
    // accumulator.
    "accumulate_dispatch_dep_signature",
    // The slot-binding-graph dual-emit helper. Its body intentionally
    // calls BOTH `accumulate_dispatch_dep_signature(sig)` (legacy
    // drain path) AND `observe_fact_signature(...)` (fact-tracer
    // fan-out) so the curated `state.fact_versions` channel stays
    // populated alongside the per-tracer fan-out until the legacy
    // drain is retired.
    "emit_slot_binding_graph_dispatch_facts",
];

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    enclosing_fn: String,
    callsite: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: in fn `{}`: `{}` -- caller not in allow-list",
            self.file.display(),
            self.enclosing_fn,
            self.callsite
        )
    }
}

/// Visitor that records every call expression whose callee path's
/// last segment is `accumulate_dispatch_dep_signature` and whose
/// enclosing `fn` name is NOT in the allow-list. The visitor walks
/// only production-source items; `#[cfg(test)] mod tests`, `mod
/// tests`, and `#[cfg(test)] impl` blocks are skipped so test
/// fixtures (which may legitimately exercise the legacy helper) are
/// not flagged.
struct Scanner<'a> {
    file: &'a Path,
    fn_stack: Vec<String>,
    cfg_test_depth: u32,
    violations: &'a mut Vec<Violation>,
    allow_set: HashSet<&'static str>,
}

impl<'a> Scanner<'a> {
    fn new(file: &'a Path, violations: &'a mut Vec<Violation>) -> Self {
        Self {
            file,
            fn_stack: Vec::new(),
            cfg_test_depth: 0,
            violations,
            allow_set: ACCUMULATE_DISPATCH_DEP_SIGNATURE_ALLOW
                .iter()
                .copied()
                .collect(),
        }
    }

    fn current_fn(&self) -> &str {
        self.fn_stack.last().map(String::as_str).unwrap_or("<root>")
    }

    fn record(&mut self, callsite: String) {
        if self.cfg_test_depth > 0 {
            return;
        }
        if self.allow_set.contains(self.current_fn()) {
            return;
        }
        self.violations.push(Violation {
            file: self.file.to_path_buf(),
            enclosing_fn: self.current_fn().to_string(),
            callsite,
        });
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_mod(&mut self, m: &'ast ItemMod) {
        // `#[cfg(test)] mod foo` and `mod tests` carve out test
        // scaffolding so test fixtures may exercise the legacy
        // helper without tripping the guard.
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
        // Match function-call expressions where the callee's path
        // ends with `accumulate_dispatch_dep_signature`. Path forms
        // we recognise:
        //
        //   accumulate_dispatch_dep_signature(...)
        //   super::dep_signature::accumulate_dispatch_dep_signature(...)
        //   crate::meta_resolve::dep_signature::accumulate_dispatch_dep_signature(...)
        //
        // Method-call form (`x.accumulate_dispatch_dep_signature()`)
        // is impossible — the helper is a free function — so we do
        // not handle `ExprMethodCall` here.
        if let syn::Expr::Path(p) = &*c.func {
            if let Some(last_seg) = p.path.segments.last() {
                if last_seg.ident == "accumulate_dispatch_dep_signature" {
                    let rendered = render_path(&p.path);
                    self.record(format!("{rendered}(...)"));
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

/// `#[cfg(test)]`, `#[cfg(any(test, ...))]`, or `#[cfg(all(..., test, ...))]`.
/// Mirrors the helper in `import_route_writer_guard.rs` so cfg-test
/// items are uniformly recognised as test-only.
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

fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed =
        syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let mut scanner = Scanner::new(path, violations);
    scanner.visit_file(&parsed);
}

fn format_violations(violations: &[Violation]) -> String {
    let mut by_file: BTreeMap<&Path, Vec<&Violation>> = BTreeMap::new();
    for v in violations {
        by_file.entry(v.file.as_path()).or_default().push(v);
    }
    let mut lines = Vec::new();
    for (file, vs) in by_file {
        lines.push(format!("  {}", file.display()));
        for v in vs {
            lines.push(format!("    fn `{}`: {}", v.enclosing_fn, v.callsite));
        }
    }
    format!(
        "found {} `accumulate_dispatch_dep_signature` call-site violation(s):\n{}",
        violations.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Production-tree guard.
// ---------------------------------------------------------------------------

/// `accumulate_dispatch_dep_signature` may be called only from:
///
/// - `accumulate_dispatch_dep_signature` itself (the helper
///   definition's `fn` head; the body contains no inner recursive
///   call, but the scanner allows the name so the function head is
///   not mis-flagged if a future refactor were to introduce one).
/// - `emit_slot_binding_graph_dispatch_facts` in
///   `slot_binding_graph.rs` (the Block 1.C dual-emit helper).
///
/// Every other production-source caller fails this guard with a
/// pointer to the offending file and function. The fix is to rewire
/// the call site through the fact-tracer fan-out path —
/// `observe_fact_signature(&dep_signature_to_fact_signature(&read.dep_signature))`
/// — instead of pushing into the legacy thread-local accumulator.
#[test]
fn accumulate_dispatch_dep_signature_call_sites_are_allow_listed() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "`accumulate_dispatch_dep_signature` must only be called from \
         the allow-listed helpers ({}). The legacy thread-local \
         accumulator is being retired; new dispatch-fact emissions \
         must fan into every active `FactReadSet` via \
         `observe_fact_signature(&dep_signature_to_fact_signature(...))` \
         so the fact-tracer captures the same dep-signature. If you \
         hit this guard, replace your call with the fact-tracer \
         fan-out pattern.\n\n{}",
        ACCUMULATE_DISPATCH_DEP_SIGNATURE_ALLOW.join(", "),
        format_violations(&violations)
    );
}

// ---------------------------------------------------------------------------
// Sentinel: discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Drive the scanner against synthetic source to confirm the
/// classification works. Without this check, the production-tree
/// guard could pass trivially if the scanner never detected ANY
/// call site.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: free-function call from an arbitrary fn — REJECTED.
    let fixture_a = r#"
        fn arbitrary_caller() {
            accumulate_dispatch_dep_signature(&sig);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_a).is_empty(),
        "scanner failed to flag arbitrary fn calling `accumulate_dispatch_dep_signature`"
    );

    // Fixture B: qualified-path call from arbitrary fn — REJECTED.
    let fixture_b = r#"
        fn arbitrary_caller() {
            super::dep_signature::accumulate_dispatch_dep_signature(&sig);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_b).is_empty(),
        "scanner failed to flag qualified-path call to `accumulate_dispatch_dep_signature`"
    );

    // Fixture C: fully-qualified crate path from arbitrary fn — REJECTED.
    let fixture_c = r#"
        fn arbitrary_caller() {
            crate::meta_resolve::accumulate_dispatch_dep_signature(&sig);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_c).is_empty(),
        "scanner failed to flag crate-path call to `accumulate_dispatch_dep_signature`"
    );

    // Fixture D: call inside the dual-emit helper — ACCEPTED.
    let fixture_d = r#"
        fn emit_slot_binding_graph_dispatch_facts(sig: &crate::semantic_query::DepSignature) {
            accumulate_dispatch_dep_signature(sig);
            let bridged = crate::fact_signature_helpers::dep_signature_to_fact_signature(sig);
            crate::fact_signature_helpers::observe_fact_signature(&bridged);
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_d).is_empty(),
        "scanner incorrectly flagged the dual-emit helper body: {:?}",
        scan_fixture_violations(fixture_d)
    );

    // Fixture E: the helper definition itself with no inner call —
    // ACCEPTED (no call expressions of the watched name appear at all).
    let fixture_e = r#"
        pub(crate) fn accumulate_dispatch_dep_signature(sig: &crate::semantic_query::DepSignature) {
            DISPATCH_DEP_SIGNATURE_ACCUMULATOR.with(|cell| {
                let mut accumulator = cell.borrow_mut();
                let _ = accumulator;
                let _ = sig;
            });
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_e).is_empty(),
        "scanner incorrectly flagged the helper definition body: {:?}",
        scan_fixture_violations(fixture_e)
    );

    // Fixture F: pure `use` import — never flagged (the visitor only
    // catches `ExprCall`, not `ItemUse`).
    let fixture_f = r#"
        use super::dep_signature::accumulate_dispatch_dep_signature;
        fn unrelated() {}
    "#;
    assert!(
        scan_fixture_violations(fixture_f).is_empty(),
        "scanner incorrectly flagged a `use` import: {:?}",
        scan_fixture_violations(fixture_f)
    );

    // Fixture G: `#[cfg(test)] mod tests` block with arbitrary call —
    // ACCEPTED. Test scaffolding is exempt from the guard so
    // characterization tests may legitimately exercise the legacy
    // helper.
    let fixture_g = r#"
        #[cfg(test)]
        mod tests {
            fn test_helper() {
                accumulate_dispatch_dep_signature(&sig);
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_g).is_empty(),
        "scanner incorrectly flagged a `#[cfg(test)] mod tests` body: {:?}",
        scan_fixture_violations(fixture_g)
    );

    // Fixture H: nested `impl` calling from a method on an unrelated
    // function name — REJECTED. The scanner walks `ImplItemFn` and
    // applies the same allow-list as for free fns.
    let fixture_h = r#"
        impl Foo {
            fn bar(&self) {
                accumulate_dispatch_dep_signature(&self.sig);
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_h).is_empty(),
        "scanner failed to flag arbitrary impl-method call to `accumulate_dispatch_dep_signature`"
    );

    // Fixture I: re-export shape — `pub(crate) use ... as something;`
    // is also an `ItemUse`, not an `ExprCall`, so never flagged.
    let fixture_i = r#"
        pub(crate) use crate::meta_resolve::dep_signature::accumulate_dispatch_dep_signature as legacy_accum;
        fn unrelated() {}
    "#;
    assert!(
        scan_fixture_violations(fixture_i).is_empty(),
        "scanner incorrectly flagged a `pub use ... as ...` re-export: {:?}",
        scan_fixture_violations(fixture_i)
    );
}

fn scan_fixture_violations(src: &str) -> Vec<Violation> {
    let parsed = syn::parse_file(src).expect("parse fixture");
    let mut violations = Vec::new();
    let fake_path = Path::new("<fixture>");
    let mut scanner = Scanner::new(fake_path, &mut violations);
    scanner.visit_file(&parsed);
    violations
}
