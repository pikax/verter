//! Architecture guard — fact-validated caches in production source
//! must admit through `insert_arc_with_kind` (NOT the loose
//! `insert_arc` / `insert` paths) unless explicitly allow-listed.
//!
//! ## Why
//!
//! `ValidatedFactCache::insert_arc_with_kind(key, value, facts, kind)`
//! is the strict admission path defined by the Block 0 substrate.
//! It enforces R20 fact-completeness: an empty `facts` vector causes
//! admission to be refused (the cache entry is not recorded, the
//! `admission_refused_count()` counter advances, and a
//! `FactSignatureAdmissionRefused { cache_kind: <kind> }` structured
//! audit event fires). Over-cap signatures (more than
//! `FACT_SIGNATURE_CAP = 1024` entries) are similarly refused via the
//! `signature_overflow_count()` path.
//!
//! The loose `insert_arc` and `insert` (which falls through to
//! `insert_arc`) paths bypass this gate. Block 0 documents that
//! `insert_arc` is reserved for "stable-miss producers" — a small
//! number of legacy call sites where an empty-signature admission is
//! variant-gated or non-empty-signature-gated at the call site
//! itself, NOT by the cache substrate. Every NEW fact-validated
//! cache admission must use the strict path.
//!
//! ## What this guard does
//!
//! Scans `crates/verter_session/src/**/*.rs` (excluding sibling
//! `*_tests.rs`, `tests/`, `benches/`, `examples/`, and
//! `#[cfg(test)]` items) for method-call expressions whose method
//! name is `insert_arc` or `insert` AND whose receiver is a
//! `ValidatedFactCache` (recognised structurally by the field name
//! pattern). Allow-listed call sites are documented below.
//!
//! ## Allow-list
//!
//! Seven pre-existing call sites in production source legitimately
//! use the loose `<receiver>.insert(...)` / `<receiver>.insert_arc(...)`
//! admission path. Three categories — call-site-gated, helper
//! method, and wrapper method — are all documented in the Block
//! 1.7 audit at
//! `D:/tmp/block1.7-facts-irrelevant-eligibility.md`. Adding a new
//! call site here requires extending the allow-list AND extending
//! the audit doc.
//!
//! ### Category A: call-site-gated cold paths (variant or fact-presence)
//!
//! - `FallthroughResolverState::store_node` in
//!   `resolver_core/fallthrough_resolver.rs`. The cold-store path
//!   admits only when `!result.facts.is_empty()` OR the value is
//!   one of the inherently-constant variants
//!   (`IntrinsicSurface(_)` / `ConsumedBindings(_)`) — the latter is
//!   the canonical `is_facts_irrelevant: true` candidate identified
//!   by the Block 1.7 audit.
//! - `FallthroughResolverState::resolve_node` in
//!   `resolver_core/fallthrough_resolver.rs`. The singleflight cold
//!   body applies the same variant-or-facts gate via a local
//!   `stable` boolean before `self.cache.insert(...)`. Same
//!   audit-eligibility as `store_node`.
//! - `SymbolResolverState::resolve_node` in
//!   `resolver_core/symbol_resolver.rs`. The singleflight cold body
//!   admits only when `!result.facts.is_empty()` (the local
//!   `stable` boolean). Always non-empty in production today; the
//!   migration to `insert_arc_with_kind` is in-flight per the
//!   audit follow-up section.
//!
//! ### Category B: pre-resolved helper inserters
//!
//! - `RouteDb::insert_route_with_facts` and `RouteDb::insert_barrel_surface`
//!   in `resolver_core/route_db.rs`. Helpers that accept a pre-
//!   built `RouteResult` / `BarrelRouteSurface` plus its
//!   `facts: Vec<FactVersionRef>`. Used exclusively by tests in
//!   `crates/verter_session/tests/` to seed the cache before
//!   exercising the warm-hit + fact-bubble path. Live in production
//!   source (not `#[cfg(test)]`) because they are part of the
//!   public `RouteDb` API surface.
//! - `ImportedRootDb::insert_with_facts` in
//!   `resolver_core/imported_root_db.rs`. Mirror of the route_db
//!   helpers; currently has no callers (kept for future test-helper
//!   parity).
//!
//! ### Category C: wrapper methods (pass-through)
//!
//! - `StableRequestState::insert_arc` in
//!   `resolver_core/resolver_runtime.rs`. The wrapper method's body
//!   is a single `self.cache.insert_arc(...)` line — the wrapper
//!   itself routes loose-admission calls from the helper-API
//!   surface (Category B) to the substrate. The strict-admission
//!   wrapper `insert_arc_with_kind` (defined a few lines below) is
//!   what production cold producers actually call.
//!
//! Every other production-source call site MUST go through
//! `insert_arc_with_kind` with a `'static str` cache-kind label.
//!
//! ## Production cache kinds passed to `insert_arc_with_kind`
//!
//! The kinds enumerated below match the runtime test in
//! `r20_admission_refuses_empty_signature.rs`. Both lists must stay
//! in lockstep.
//!
//! - `"prepared_decl_bundles"` (`host_manage/prepared_decl.rs`)
//! - `"component_meta.results"` (`host_manage/component_meta_methods.rs`)
//! - `"imported_root_db.roots"` (`resolver_core/imported_root_db.rs`)
//! - `"route_db.routes"` (`resolver_core/route_db.rs`)
//! - `"route_db.barrel_surfaces"` (`resolver_core/route_db.rs`)
//! - `"route_db.effective_export_sets"` (`resolver_core/route_db.rs`)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, ExprMethodCall, ImplItemFn, ItemFn, ItemImpl, ItemMod, Meta};
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

/// Allow-listed `(file_suffix, enclosing_fn)` pairs whose
/// `<receiver>.insert(...)` / `<receiver>.insert_arc(...)` call
/// site is permitted to bypass strict admission. The file-suffix
/// match is path-suffix (`ends_with`) so the entries are platform-
/// agnostic and survive worktree path changes. See module
/// docstring for the per-entry justification (audit categories
/// A/B/C).
const LOOSE_ADMISSION_ALLOW_LIST: &[(&str, &str)] = &[
    // Category A: variant-gated / fact-presence-gated cold paths.
    ("resolver_core/fallthrough_resolver.rs", "store_node"),
    ("resolver_core/fallthrough_resolver.rs", "resolve_node"),
    ("resolver_core/symbol_resolver.rs", "resolve_node"),
    // Category B: pre-resolved helper inserters (test-only callers).
    ("resolver_core/route_db.rs", "insert_route_with_facts"),
    ("resolver_core/route_db.rs", "insert_barrel_surface"),
    ("resolver_core/imported_root_db.rs", "insert_with_facts"),
    // Category C: substrate wrapper method (pass-through).
    ("resolver_core/resolver_runtime.rs", "insert_arc"),
];

/// Production-source cache kinds. Mirrors
/// `PRODUCTION_CACHE_KINDS` in `r20_admission_refuses_empty_signature.rs`.
/// Used to assert every kind below is actually present somewhere in
/// the source tree (a regression that renamed a kind without
/// updating this list would fail the discriminator).
const PRODUCTION_CACHE_KINDS: &[&str] = &[
    "prepared_decl_bundles",
    "component_meta.results",
    "imported_root_db.roots",
    "route_db.routes",
    "route_db.barrel_surfaces",
    "route_db.effective_export_sets",
];

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    enclosing_fn: String,
    method: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: in fn `{}`: `.{}(...)` call on a `ValidatedFactCache`-shaped receiver",
            self.file.display(),
            self.enclosing_fn,
            self.method
        )
    }
}

struct Scanner<'a> {
    file: &'a Path,
    fn_stack: Vec<String>,
    cfg_test_depth: u32,
    violations: &'a mut Vec<Violation>,
    allow_list: &'a [(&'static str, &'static str)],
}

impl<'a> Scanner<'a> {
    fn new(
        file: &'a Path,
        violations: &'a mut Vec<Violation>,
        allow_list: &'a [(&'static str, &'static str)],
    ) -> Self {
        Self {
            file,
            fn_stack: Vec::new(),
            cfg_test_depth: 0,
            violations,
            allow_list,
        }
    }

    fn current_fn(&self) -> &str {
        self.fn_stack.last().map(String::as_str).unwrap_or("<root>")
    }

    fn is_allow_listed(&self, file: &Path, enclosing_fn: &str) -> bool {
        let p = file.to_string_lossy().replace('\\', "/");
        self.allow_list
            .iter()
            .any(|(suffix, fn_name)| p.ends_with(*suffix) && enclosing_fn == *fn_name)
    }

    fn record(&mut self, method: &str) {
        if self.cfg_test_depth > 0 {
            return;
        }
        if self.is_allow_listed(self.file, self.current_fn()) {
            return;
        }
        self.violations.push(Violation {
            file: self.file.to_path_buf(),
            enclosing_fn: self.current_fn().to_string(),
            method: method.to_string(),
        });
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

    fn visit_expr_method_call(&mut self, c: &'ast ExprMethodCall) {
        let method = c.method.to_string();
        // We watch only `insert_arc` (loose admission on
        // `ValidatedFactCache`) and `insert` (wraps `insert_arc`).
        // Receivers we recognise as `ValidatedFactCache`-shaped:
        //   - `self.cache.insert(...)` / `self.cache.insert_arc(...)`
        //   - `self.<fieldname>.insert(...)` / `.insert_arc(...)`
        //     where `<fieldname>` is one of the known fact-validated
        //     fields. We syntactically approximate by matching any
        //     `self.<ident>.<method>(...)` call where the immediate
        //     receiver chain starts with `self.<ident>` and the
        //     enclosing function is not allow-listed AND the field
        //     name is a known `ValidatedFactCache` field.
        if (method == "insert_arc" || method == "insert")
            && receiver_is_known_validated_fact_cache_field(&c.receiver)
        {
            self.record(&method);
        }
        syn::visit::visit_expr_method_call(self, c);
    }
}

/// Known `ValidatedFactCache`-typed fields on production structs.
/// Used to disambiguate `<receiver>.insert(...)` calls so that
/// unrelated `Vec::insert`, `HashMap::insert`, and
/// `DashMap::insert` calls are not flagged.
const VALIDATED_FACT_CACHE_FIELDS: &[&str] = &[
    "cache",
    "roots",
    "routes",
    "barrel_surfaces",
    "effective_export_sets",
    "prepared_decl_bundles",
    "component_meta",
];

/// Match `self.<known_field>` or `<binding>.<known_field>` receivers.
/// We do not chase deep field chains — the production call sites we
/// guard against are uniformly `self.<field>.insert(...)` /
/// `self.<field>.insert_arc(...)` (or, inside `StableRequestState`,
/// `self.cache.insert*` and the analogous `FallthroughResolverState`
/// / `SymbolResolverState` patterns).
fn receiver_is_known_validated_fact_cache_field(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Field(f) => {
            let name = match &f.member {
                syn::Member::Named(i) => i.to_string(),
                syn::Member::Unnamed(_) => return false,
            };
            VALIDATED_FACT_CACHE_FIELDS.contains(&name.as_str())
        }
        syn::Expr::Paren(p) => receiver_is_known_validated_fact_cache_field(&p.expr),
        syn::Expr::Reference(r) => receiver_is_known_validated_fact_cache_field(&r.expr),
        _ => false,
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

fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed =
        syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let mut scanner = Scanner::new(path, violations, LOOSE_ADMISSION_ALLOW_LIST);
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
            lines.push(format!("    fn `{}`: `.{}(...)`", v.enclosing_fn, v.method));
        }
    }
    format!(
        "found {} loose-admission call site violation(s):\n{}",
        violations.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Production-tree guard.
// ---------------------------------------------------------------------------

/// Fact-validated caches in `crates/verter_session/src/**/*.rs` admit
/// through `insert_arc_with_kind` only, EXCEPT for the documented
/// allow-list in `LOOSE_ADMISSION_ALLOW_LIST`. Adding a new
/// fact-validated cache producer that calls `.insert(...)` or
/// `.insert_arc(...)` outside the allow-list fails this guard with
/// a pointer to the offending file and enclosing function.
#[test]
fn fact_validated_caches_use_strict_admission_in_production_source() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "fact-validated caches must admit through `insert_arc_with_kind` \
         (not `insert` / `insert_arc`). The strict path enforces R20 \
         empty-signature refusal. Allow-listed legacy producers are:\n  - {}\n\n\
         Fix: rewrite the call to \
         `<cache>.insert_arc_with_kind(key, value, facts, \"<cache_kind>\")` \
         and extend `r20_admission_refuses_empty_signature.rs` with the new \
         cache-kind literal. If the producer needs to admit with empty facts \
         (an `is_facts_irrelevant`-eligible case per the Block 1.7 audit), \
         extend `LOOSE_ADMISSION_ALLOW_LIST` after adding the audit \
         justification to `D:/tmp/block1.7-facts-irrelevant-eligibility.md`.\n\n{}",
        LOOSE_ADMISSION_ALLOW_LIST
            .iter()
            .map(|(f, fn_name)| format!("{f}::{fn_name}"))
            .collect::<Vec<_>>()
            .join("\n  - "),
        format_violations(&violations)
    );
}

/// Pin every kind in `PRODUCTION_CACHE_KINDS` to at least one
/// occurrence in the production source tree. A regression that
/// renamed or removed a kind without updating both this list and
/// `r20_admission_refuses_empty_signature.rs` would fail here.
#[test]
fn every_production_cache_kind_is_present_in_source() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut all_source = String::new();
    for file in walk_production_rs_files(&crate_root) {
        match std::fs::read_to_string(&file) {
            Ok(s) => all_source.push_str(&s),
            Err(_) => continue,
        }
    }
    for kind in PRODUCTION_CACHE_KINDS {
        let needle = format!("\"{kind}\"");
        assert!(
            all_source.contains(&needle),
            "production cache kind `{kind}` not found in any production-source file. \
             Either the kind was renamed (update both PRODUCTION_CACHE_KINDS lists) \
             or the producer was deleted (drop the kind from the lists). \
             Lists to update: \
             `insert_arc_strict_admission_required.rs::PRODUCTION_CACHE_KINDS` AND \
             `r20_admission_refuses_empty_signature.rs::PRODUCTION_CACHE_KINDS`."
        );
    }
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
    // Fixture A: `self.cache.insert(...)` from an unrelated fn — REJECTED.
    let fixture_a = r#"
        fn arbitrary_caller(this: &Foo) {
            this.cache.insert(k, v, facts);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_a, LOOSE_ADMISSION_ALLOW_LIST).is_empty(),
        "scanner failed to flag `self.cache.insert(...)` in arbitrary fn"
    );

    // Fixture B: `self.roots.insert_arc(...)` from an unrelated fn — REJECTED.
    let fixture_b = r#"
        fn arbitrary_caller(this: &Foo) {
            this.roots.insert_arc(k, v, facts);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_b, LOOSE_ADMISSION_ALLOW_LIST).is_empty(),
        "scanner failed to flag `self.roots.insert_arc(...)`"
    );

    // Fixture C: `self.routes.insert_arc_with_kind(...)` — ACCEPTED.
    let fixture_c = r#"
        fn arbitrary_caller(this: &Foo) {
            this.routes.insert_arc_with_kind(k, v, facts, "route_db.routes");
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_c, LOOSE_ADMISSION_ALLOW_LIST).is_empty(),
        "scanner incorrectly flagged `insert_arc_with_kind`: {:?}",
        scan_fixture_violations(fixture_c, LOOSE_ADMISSION_ALLOW_LIST)
    );

    // Fixture D: `vec.insert(...)` (unrelated `Vec::insert`) — ACCEPTED.
    let fixture_d = r#"
        fn arbitrary_caller(v: &mut Vec<u32>) {
            v.insert(0, 42);
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_d, LOOSE_ADMISSION_ALLOW_LIST).is_empty(),
        "scanner incorrectly flagged `Vec::insert`: {:?}",
        scan_fixture_violations(fixture_d, LOOSE_ADMISSION_ALLOW_LIST)
    );

    // Fixture E: `map.insert(...)` (DashMap / HashMap field NOT in the
    // known-fields list) — ACCEPTED. The scanner only watches the
    // listed fact-validated-cache field names.
    let fixture_e = r#"
        fn arbitrary_caller(this: &Foo) {
            this.unrelated_map.insert(k, v);
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_e, LOOSE_ADMISSION_ALLOW_LIST).is_empty(),
        "scanner incorrectly flagged unrelated map insert: {:?}",
        scan_fixture_violations(fixture_e, LOOSE_ADMISSION_ALLOW_LIST)
    );

    // Fixture F: call inside an allow-listed fn — ACCEPTED. We use
    // the discriminator-only fixture path (the scanner's allow-list
    // is by `(file_suffix, enclosing_fn)`, and the synthetic file
    // path is `<fixture>` which does not match any allow-list
    // suffix — so allow-list does not fire here, and the call IS
    // flagged. We invert the assertion to confirm the unflagged
    // case requires a matching file suffix). Verified separately
    // via the `allow_list_recognises_documented_call_sites` test.
    let fixture_f = r#"
        fn store_node(this: &Foo) {
            this.cache.insert(k, v, facts);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_f, LOOSE_ADMISSION_ALLOW_LIST).is_empty(),
        "scanner should flag `store_node` when path does NOT match allow-list \
         file suffix (synthetic fixture path is `<fixture>`)"
    );

    // Fixture G: `#[cfg(test)] mod tests` body — ACCEPTED. Test
    // scaffolding is exempt.
    let fixture_g = r#"
        #[cfg(test)]
        mod tests {
            fn helper(this: &Foo) {
                this.cache.insert(k, v, facts);
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_g, LOOSE_ADMISSION_ALLOW_LIST).is_empty(),
        "scanner incorrectly flagged `#[cfg(test)] mod tests` body: {:?}",
        scan_fixture_violations(fixture_g, LOOSE_ADMISSION_ALLOW_LIST)
    );
}

/// The allow-list recognises the two documented call sites via
/// path-suffix matching. We exercise the allow-list logic directly
/// (bypassing the synthetic-fixture path mismatch) by constructing
/// a real-path PathBuf that ends with the allow-listed suffix.
#[test]
fn allow_list_recognises_documented_call_sites() {
    let mut violations = Vec::new();
    let real_path = PathBuf::from(
        "/some/prefix/crates/verter_session/src/resolver_core/fallthrough_resolver.rs",
    );
    let scanner = Scanner::new(&real_path, &mut violations, LOOSE_ADMISSION_ALLOW_LIST);
    assert!(
        scanner.is_allow_listed(&real_path, "store_node"),
        "allow-list must match (`resolver_core/fallthrough_resolver.rs`, `store_node`)"
    );
    let real_path2 =
        PathBuf::from("/some/prefix/crates/verter_session/src/resolver_core/symbol_resolver.rs");
    let scanner2 = Scanner::new(&real_path2, &mut violations, LOOSE_ADMISSION_ALLOW_LIST);
    assert!(
        scanner2.is_allow_listed(&real_path2, "resolve_node"),
        "allow-list must match (`resolver_core/symbol_resolver.rs`, `resolve_node`)"
    );
    // Non-matching fn name on a matching file — NOT allow-listed.
    assert!(
        !scanner2.is_allow_listed(&real_path2, "arbitrary_other_fn"),
        "allow-list must NOT match a non-listed fn on an allow-listed file"
    );
    // Matching fn name on a non-matching file — NOT allow-listed.
    let real_path3 = PathBuf::from("/some/prefix/crates/verter_session/src/lib.rs");
    let scanner3 = Scanner::new(&real_path3, &mut violations, LOOSE_ADMISSION_ALLOW_LIST);
    assert!(
        !scanner3.is_allow_listed(&real_path3, "store_node"),
        "allow-list must NOT match a listed fn name on a non-listed file"
    );
}

fn scan_fixture_violations(
    src: &str,
    allow_list: &[(&'static str, &'static str)],
) -> Vec<Violation> {
    let parsed = syn::parse_file(src).expect("parse fixture");
    let mut violations = Vec::new();
    let fake_path = Path::new("<fixture>");
    let mut scanner = Scanner::new(fake_path, &mut violations, allow_list);
    scanner.visit_file(&parsed);
    violations
}
