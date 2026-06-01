//! Architecture guard — no eager bulk invalidation outside dedicated
//! reset/clear methods.
//!
//! The fact-based cache architecture (R1–R29) admits cache entries
//! under content-addressed or query-identity keys, then revalidates on
//! warm hits via `HostFenceValidator` and dep-signatures. Eager bulk
//! invalidation (`self.cache.clear()` outside a dedicated reset
//! method, or `cache.iter_mut().for_each(|_| .invalidate(...))` /
//! `cache.iter_mut().for_each(|_| .clear())`) bypasses that machinery
//! and turns the cache into a write-through store with no version
//! semantics.
//!
//! Targeted, canonical-keyed invalidation (`invalidate_canonical(id)`
//! / `invalidate_for_canonical(id)`) is the documented narrow API and
//! is NEVER flagged.
//!
//! What the guard rejects:
//!
//! - `self.<cache_field>.clear()` calls in production source whose
//!   enclosing function is NOT in the allow-list of dedicated
//!   reset/clear methods (`clear_*`, `reset_*`, `drop_*`, `purge_*`,
//!   `wipe_*`, `configure_projects`, `finish_upsert_post_commit`).
//! - `self.<cache_field>.iter_mut().for_each(...)` calls in production
//!   source — bulk mutation across all cache entries is a strict
//!   superset of bulk clear and equally bypasses revalidation.
//! - `cache.values_mut().for_each(...)` / `cache.iter_mut().for_each(...)`
//!   outside the allow-list — same reasoning.
//!
//! Allow-list (methods whose ENTIRE purpose is a lifecycle reset):
//! - `clear_*`, `reset_*`, `drop_*`, `purge_*`, `wipe_*` — any free
//!   or method function whose name begins with one of these verbs is
//!   by-name dedicated to cache reset, and may call `cache.clear()` or
//!   `cache.iter_mut().for_each(...)`.
//! - `configure_projects` — lifecycle reconfiguration of the project
//!   resolver; clears caches whose validity depends on the project
//!   graph identity.
//! - `finish_upsert_post_commit` — the per-canonical post-commit step
//!   of the shared upsert engine (`upsert_many_with_priority`); drains
//!   the upserted canonical's own caches (the own-canonical drain). The
//!   reverse-dependent cascade was removed; this entry covers only the
//!   upserted file's own-cache drain.
//!
//! Reference pattern: Block 1.E's `import_route_writer_guard.rs` and
//! Block 1.G's `no_accumulate_dispatch_dep_signature_outside_helpers.rs`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, ExprMethodCall, ImplItemFn, ItemFn, ItemImpl, ItemMod, Meta};
use walkdir::WalkDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Enclosing-function name prefixes that mark a method as dedicated to
/// lifecycle reset / cache eviction. Inside such a method, a bulk
/// `cache.clear()` or `cache.iter_mut().for_each(...)` is the intent,
/// not a defect.
const RESET_NAME_PREFIXES: &[&str] = &[
    "clear_", "reset_", "drop_", "purge_", "wipe_", "evict_", "bump_",
];

/// Whole-function names allowed to perform bulk clears beyond the
/// name-prefix rule. These are documented lifecycle reset producers
/// whose names do not start with `clear_`/`reset_`.
const RESET_WHOLE_NAMES: &[&str] = &[
    // Project-graph lifecycle reset entry points.
    "configure_projects",
    // Per-canonical post-commit of the shared upsert engine
    // (`upsert_many_with_priority`) — drains the upserted canonical's own
    // caches. This is the own-canonical drain; there is no
    // reverse-dependent cascade.
    "finish_upsert_post_commit",
    // File close / eviction lifecycle.
    "close",
    "evict",
    "remove",
    // Bare `clear` — `ResolverRuntime::clear()` is the documented
    // reset surface on per-resolver runtimes (mirrors the `clear_*`
    // prefix on container types).
    "clear",
    // Workspace re-attach: `host_lifecycle::set_workspace` replaces the
    // entire workspace binding; the route-cache invalidation is part
    // of the lifecycle reset, not arbitrary mutation.
    "set_workspace",
    // Frontier / exact-resolution snapshot writer: bumps generation
    // and re-publishes routes; identical lifecycle semantics to
    // `set_import_dependencies`.
    "set_exact_resolutions",
    // The route-cache writer in `dependency_resolution.rs` may rebuild
    // the route map; the import_route_writer_guard.rs scanner already
    // pins down its precise admission contract.
    "set_import_dependencies",
];

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    enclosing_fn: String,
    op: String,
    detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: in fn `{}`: {} -- {}",
            self.file.display(),
            self.enclosing_fn,
            self.op,
            self.detail
        )
    }
}

struct Scanner<'a> {
    file: &'a Path,
    fn_stack: Vec<String>,
    cfg_test_depth: u32,
    violations: &'a mut Vec<Violation>,
}

impl<'a> Scanner<'a> {
    fn new(file: &'a Path, violations: &'a mut Vec<Violation>) -> Self {
        Self {
            file,
            fn_stack: Vec::new(),
            cfg_test_depth: 0,
            violations,
        }
    }

    fn current_fn(&self) -> &str {
        self.fn_stack.last().map(String::as_str).unwrap_or("<root>")
    }

    fn fn_is_reset_method(&self) -> bool {
        let cur = self.current_fn();
        if RESET_WHOLE_NAMES.contains(&cur) {
            return true;
        }
        RESET_NAME_PREFIXES.iter().any(|p| cur.starts_with(p))
    }

    fn record(&mut self, op: &str, detail: String) {
        if self.cfg_test_depth > 0 {
            return;
        }
        if self.fn_is_reset_method() {
            return;
        }
        self.violations.push(Violation {
            file: self.file.to_path_buf(),
            enclosing_fn: self.current_fn().to_string(),
            op: op.to_string(),
            detail,
        });
    }
}

/// True if the receiver of a `.clear()` / `.iter_mut()` / `.values_mut()`
/// chain looks like a cache-field expression. Concretely: the receiver
/// must end in a `Field` whose member identifier ends in `_cache`,
/// `_caches`, `_db`, `_store`, or is named `cache`. This catches
/// `self.compile_cache.clear()` / `self.derived_raw_cache.clear()` /
/// `self.member_display_fact_store.clear()` etc.
fn receiver_is_cache_field(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(m) => {
            // Some chains wrap a cache field through a getter method
            // (e.g. `self.compile_cache().clear()`). In that case, the
            // receiver of the inner method call is `self` and the
            // method name itself is the cache identity.
            let m_name = m.method.to_string();
            if name_is_cache_like(&m_name) {
                return true;
            }
            receiver_is_cache_field(&m.receiver)
        }
        syn::Expr::Field(f) => {
            let member = match &f.member {
                syn::Member::Named(i) => i.to_string(),
                syn::Member::Unnamed(_) => return false,
            };
            name_is_cache_like(&member)
        }
        syn::Expr::Paren(p) => receiver_is_cache_field(&p.expr),
        syn::Expr::Reference(r) => receiver_is_cache_field(&r.expr),
        _ => false,
    }
}

fn name_is_cache_like(name: &str) -> bool {
    if name == "cache" || name == "caches" {
        return true;
    }
    name.ends_with("_cache")
        || name.ends_with("_caches")
        || name.ends_with("_db")
        || name.ends_with("_store")
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

    fn visit_expr_method_call(&mut self, m: &'ast ExprMethodCall) {
        let method = m.method.to_string();
        // Bulk-clear shape: `<cache>.clear()` (zero args).
        if method == "clear" && m.args.is_empty() && receiver_is_cache_field(&m.receiver) {
            self.record(
                ".clear()",
                format!(
                    "bulk `.clear()` on cache-shaped receiver from `{}` — \
                     use `invalidate_canonical(canonical)` for targeted \
                     invalidation, or move the clear into a dedicated \
                     `clear_*`/`reset_*`/`drop_*` method",
                    self.current_fn()
                ),
            );
        }
        // Bulk mutation shape: `<cache>.iter_mut().for_each(...)` or
        // `<cache>.values_mut().for_each(...)` — equivalent to a
        // bulk clear from the cache-validity contract's standpoint.
        if method == "for_each" {
            if let syn::Expr::MethodCall(inner) = &*m.receiver {
                let inner_method = inner.method.to_string();
                if (inner_method == "iter_mut" || inner_method == "values_mut")
                    && receiver_is_cache_field(&inner.receiver)
                {
                    self.record(
                        &format!(".{inner_method}().for_each()"),
                        format!(
                            "bulk mutation chain `.{inner_method}().for_each(...)` \
                             on cache-shaped receiver from `{}` — equivalent to \
                             a bulk clear; use targeted `invalidate_canonical(id)` \
                             or move into a dedicated `clear_*`/`reset_*` method",
                            self.current_fn()
                        ),
                    );
                }
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
}

/// Mirrors `import_route_writer_guard::has_cfg_test`.
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
            lines.push(format!(
                "    fn `{}`: {} -- {}",
                v.enclosing_fn, v.op, v.detail
            ));
        }
    }
    format!(
        "found {} eager-invalidation violation(s):\n{}",
        violations.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Production-tree guard.
// ---------------------------------------------------------------------------

/// Eager bulk invalidation patterns (`<cache>.clear()` or
/// `<cache>.iter_mut().for_each(...)`) are forbidden outside dedicated
/// reset/clear methods. The fix is one of:
///
/// - Move the bulk clear into a method whose name begins with
///   `clear_*` / `reset_*` / `drop_*` / `purge_*` / `wipe_*` — the
///   intent is then explicit and the method is itself revalidated.
/// - Replace the bulk operation with targeted
///   `invalidate_canonical(canonical_id)` / `invalidate_for_canonical(id)`
///   calls — fact-based revalidation only invalidates entries whose
///   `HostFenceValidator` dep-signature actually changed.
/// - Allow-list the producer in `RESET_WHOLE_NAMES` if it is a documented
///   lifecycle reset entry point.
#[test]
fn no_bulk_invalidation_outside_reset_methods() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "Block 5 `no_eager_invalidation` violation:\n{}\n\n\
         The fact-based cache architecture revalidates warm hits via \n\
         `HostFenceValidator` + dep-signatures. Eager bulk-clear bypasses \n\
         that machinery. If you need lifecycle reset, name the method \n\
         `clear_*`/`reset_*`/`drop_*`/`purge_*`/`wipe_*` so the intent is \n\
         explicit (or allow-list a documented entry point). For per-file \n\
         invalidation, use `invalidate_canonical(id)` /\n\
         `invalidate_for_canonical(id)`.",
        format_violations(&violations)
    );
}

// ---------------------------------------------------------------------------
// Sentinel: discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Discriminating-property test — drive the scanner against synthetic
/// source. Without this check, the production-tree guard could pass
/// trivially.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: arbitrary method calls `self.cache.clear()` — REJECTED.
    let fixture_a = r#"
        impl Foo {
            fn arbitrary_writer(&self) {
                self.cache.clear();
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_a).is_empty(),
        "scanner failed to flag arbitrary fn bulk-clearing `self.cache`"
    );

    // Fixture B: method whose name starts with `clear_` — ACCEPTED.
    let fixture_b = r#"
        impl Foo {
            fn clear_caches(&self) {
                self.cache.clear();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_b).is_empty(),
        "scanner incorrectly flagged a `clear_caches` reset method: {:?}",
        scan_fixture_violations(fixture_b)
    );

    // Fixture C: method whose name starts with `reset_` — ACCEPTED.
    let fixture_c = r#"
        impl Foo {
            fn reset_all_caches(&self) {
                self.compile_cache.clear();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_c).is_empty(),
        "scanner incorrectly flagged a `reset_*` method: {:?}",
        scan_fixture_violations(fixture_c)
    );

    // Fixture D: `configure_projects` (whole-name allow-list) — ACCEPTED.
    let fixture_d = r#"
        impl Foo {
            fn configure_projects(&self) {
                self.derived_raw_cache.clear();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_d).is_empty(),
        "scanner incorrectly flagged `configure_projects`: {:?}",
        scan_fixture_violations(fixture_d)
    );

    // Fixture E: `iter_mut().for_each()` bulk mutation — REJECTED.
    let fixture_e = r#"
        impl Foo {
            fn arbitrary_iter(&self) {
                self.compile_cache.iter_mut().for_each(|mut e| e.touch());
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_e).is_empty(),
        "scanner failed to flag arbitrary `iter_mut().for_each()` chain"
    );

    // Fixture F: `values_mut().for_each()` bulk mutation — REJECTED.
    let fixture_f = r#"
        impl Foo {
            fn arbitrary_values_mut(&self) {
                self.cache.values_mut().for_each(|v| *v = Default::default());
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_f).is_empty(),
        "scanner failed to flag arbitrary `values_mut().for_each()` chain"
    );

    // Fixture G: targeted invalidation — never flagged (no `.clear()`).
    let fixture_g = r#"
        impl Foo {
            fn arbitrary_targeted(&self) {
                self.cache.invalidate_canonical("file.vue");
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_g).is_empty(),
        "scanner incorrectly flagged a targeted `invalidate_canonical`: {:?}",
        scan_fixture_violations(fixture_g)
    );

    // Fixture H: `#[cfg(test)]` test module — ACCEPTED (test scaffolding).
    let fixture_h = r#"
        #[cfg(test)]
        mod tests {
            fn arbitrary_test_writer() {
                cache.clear();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_h).is_empty(),
        "scanner incorrectly flagged a #[cfg(test)] mod body: {:?}",
        scan_fixture_violations(fixture_h)
    );

    // Fixture I: `.clear()` on a non-cache field — never flagged.
    let fixture_i = r#"
        impl Foo {
            fn arbitrary_clear_pending(&self) {
                self.pending_jobs.clear();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_i).is_empty(),
        "scanner incorrectly flagged a non-cache field clear: {:?}",
        scan_fixture_violations(fixture_i)
    );

    // Fixture J: `clear_*` method calls cache.clear via method getter
    // (`self.compile_cache().clear()`) — ACCEPTED.
    let fixture_j = r#"
        impl Foo {
            fn clear_compile_cache(&self) {
                self.compile_cache().clear();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_j).is_empty(),
        "scanner incorrectly flagged a clear_* method invoking a cache getter: {:?}",
        scan_fixture_violations(fixture_j)
    );

    // Fixture K: arbitrary method bulk-clears through cache getter — REJECTED.
    let fixture_k = r#"
        impl Foo {
            fn arbitrary_getter_clear(&self) {
                self.compile_cache().clear();
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_k).is_empty(),
        "scanner failed to flag arbitrary fn calling cache_getter.clear()"
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
