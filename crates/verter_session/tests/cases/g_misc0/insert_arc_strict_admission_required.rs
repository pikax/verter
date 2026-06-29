//! Architecture guard — fact-validated caches in production source
//! must admit through `insert_arc_with_kind` (NOT the loose
//! `insert_arc` / `insert` paths) unless explicitly allow-listed.
//!
//! ## Why
//!
//! `ValidatedFactCache::insert_arc_with_kind(key, value, facts, kind)`
//! is the strict admission path.
//! It enforces R20 fact-completeness: an empty `facts` vector causes
//! admission to be refused (the cache entry is not recorded, the
//! `admission_refused_count()` counter advances, and a
//! `FactSignatureAdmissionRefused { cache_kind: <kind> }` structured
//! audit event fires). Over-cap signatures (more than
//! `FACT_SIGNATURE_CAP = 1024` entries) are similarly refused via the
//! `signature_overflow_count()` path.
//!
//! The loose `insert_arc` and `insert` (which falls through to
//! `insert_arc`) paths bypass this gate.
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
//! pattern). The scanner collects ALL such call sites; the
//! production-tree assertion compares the observed
//! `(file_suffix, fn) -> count` map against
//! `EXPECTED_LOOSE_ADMISSION_COUNTS` and fails on any deviation —
//! a surplus loose call in an allow-listed function, a deficit
//! (an entry was removed without updating the list), or a brand-
//! new `(file_suffix, fn)` pair all trip the guard.
//!
//! ## Allow-list
//!
//! Six call sites in production source legitimately
//! use the loose `<receiver>.insert(...)` / `<receiver>.insert_arc(...)`
//! admission path. Three categories — call-site-gated, helper
//! method, and wrapper method — are documented below. Adding a new
//! call site here requires extending the allow-list.
//!
//! ### Category A: call-site-gated cold paths (variant or fact-presence)
//!
//! - `FallthroughResolverState::store_node` in
//!   `resolver_core/fallthrough_resolver.rs`. The cold-store path
//!   admits only when `!result.facts.is_empty()` OR the value is
//!   one of the inherently-constant variants
//!   (`IntrinsicSurface(_)` / `ConsumedBindings(_)`) — the latter is
//!   the canonical `is_facts_irrelevant: true` candidate.
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
//! in lockstep — the cache-kind parity test (below) compares the
//! observed set of literals passed as the fourth argument to
//! `insert_arc_with_kind` against this expected set and fails on
//! any missing or unexpected kind.
//!
//! - `"prepared_decl_bundles"` (`host_manage/prepared_decl.rs`)
//! - `"component_meta.results"` (`host_manage/component_meta_methods.rs`)
//! - `"imported_root_db.roots"` (`resolver_core/imported_root_db.rs`)
//! - `"route_db.routes"` (`resolver_core/route_db.rs`)
//! - `"route_db.barrel_surfaces"` (`resolver_core/route_db.rs`)
//! - `"route_db.effective_export_sets"` (`resolver_core/route_db.rs`)

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, Expr, ExprMethodCall, ImplItemFn, ItemFn, ItemImpl, ItemMod, Lit, Meta};
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

/// Pinned `(file_suffix, enclosing_fn, expected_loose_call_count)`
/// triples whose `<receiver>.insert(...)` / `<receiver>.insert_arc(...)`
/// call site is permitted to bypass strict admission. The file-suffix
/// match is path-suffix (`ends_with`) so the entries are platform-
/// agnostic and survive worktree path changes. See module
/// docstring for the per-entry justification (audit categories
/// A/B/C).
///
/// The third tuple element is the EXACT number of loose-admission
/// call expressions the scanner expects to find inside the named
/// function. Any deviation — a surplus call (e.g. a new
/// `self.cache.insert(...)` added inside `resolve_node`), a deficit
/// (an entry removed without updating the list), or an entirely new
/// `(file_suffix, fn)` pair surfacing as a loose call — trips the
/// production-tree guard.
const EXPECTED_LOOSE_ADMISSION_COUNTS: &[(&str, &str, usize)] = &[
    // Category A: variant-gated / fact-presence-gated cold paths.
    ("resolver_core/fallthrough_resolver.rs", "store_node", 1),
    ("resolver_core/symbol_resolver.rs", "resolve_node", 1),
    // Category B: pre-resolved helper inserters (test-only callers).
    ("resolver_core/route_db.rs", "insert_route_with_facts", 1),
    ("resolver_core/route_db.rs", "insert_barrel_surface", 1),
    ("resolver_core/imported_root_db.rs", "insert_with_facts", 1),
    // Category C: substrate wrapper method (pass-through).
    ("resolver_core/resolver_runtime.rs", "insert_arc", 1),
];

/// Production-source cache kinds. Mirrors
/// `PRODUCTION_CACHE_KINDS` in `r20_admission_refuses_empty_signature.rs`.
/// The cache-kind parity test (below) asserts the observed set of
/// `insert_arc_with_kind(..., "<kind>")` literals in production
/// source is exactly equal to this list — missing kinds (a producer
/// was deleted or renamed) AND unexpected kinds (a new producer was
/// added without updating the list) both fail.
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

    fn record(&mut self, method: &str) {
        if self.cfg_test_depth > 0 {
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
        //     field name is a known `ValidatedFactCache` field.
        if (method == "insert_arc" || method == "insert")
            && receiver_is_known_validated_fact_cache_field(&c.receiver)
        {
            self.record(&method);
        }
        syn::visit::visit_expr_method_call(self, c);
    }
}

/// Walks production source and collects every string-literal
/// passed as the fourth positional argument to
/// `.insert_arc_with_kind(key, value, facts, "<cache_kind>")`. The
/// `cache_kind` parameter is declared as `&'static str` on the
/// substrate; non-literal arguments are unreachable in practice
/// and surface here as a separate violation (the kind text would
/// not be parity-checkable).
struct CacheKindCollector<'a> {
    cfg_test_depth: u32,
    kinds: &'a mut BTreeSet<String>,
    non_literal_kinds: &'a mut Vec<(PathBuf, String)>,
    file: PathBuf,
}

impl<'a> CacheKindCollector<'a> {
    fn new(
        file: PathBuf,
        kinds: &'a mut BTreeSet<String>,
        non_literal_kinds: &'a mut Vec<(PathBuf, String)>,
    ) -> Self {
        Self {
            cfg_test_depth: 0,
            kinds,
            non_literal_kinds,
            file,
        }
    }
}

impl<'ast> Visit<'ast> for CacheKindCollector<'_> {
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
        syn::visit::visit_item_fn(self, f);
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
        let entered_test = has_cfg_test(&f.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        syn::visit::visit_impl_item_fn(self, f);
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_expr_method_call(&mut self, c: &'ast ExprMethodCall) {
        if self.cfg_test_depth == 0 && c.method == "insert_arc_with_kind" {
            // The substrate signature is
            //   insert_arc_with_kind(key, value, facts, cache_kind: &'static str)
            // so the cache-kind is the 4th positional argument
            // (index 3). The forwarding wrapper on
            // `StableRequestState` has the same shape, so we count
            // both. The substrate definition itself is the only
            // call site where the 4th argument is a `&'static str`
            // parameter rather than a literal — we filter that out
            // by checking for a literal.
            if let Some(arg) = c.args.iter().nth(3) {
                match arg {
                    Expr::Lit(lit) => {
                        if let Lit::Str(s) = &lit.lit {
                            self.kinds.insert(s.value());
                        } else {
                            self.non_literal_kinds
                                .push((self.file.clone(), "non-string literal".to_string()));
                        }
                    }
                    Expr::Path(_) => {
                        // Internal forwarding: the substrate's own
                        // `insert_arc_with_kind` body forwards
                        // through the parameter `cache_kind`; that
                        // is not a producer, skip silently.
                    }
                    _ => {
                        self.non_literal_kinds.push((
                            self.file.clone(),
                            format!("non-literal expr in 4th arg: {:?}", quote_kind(arg)),
                        ));
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, c);
    }
}

fn quote_kind(e: &Expr) -> &'static str {
    match e {
        Expr::Lit(_) => "Lit",
        Expr::Path(_) => "Path",
        Expr::Call(_) => "Call",
        Expr::MethodCall(_) => "MethodCall",
        Expr::Reference(_) => "Reference",
        _ => "Other",
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
    let mut scanner = Scanner::new(path, violations);
    scanner.visit_file(&parsed);
}

fn scan_file_for_cache_kinds(
    path: &Path,
    kinds: &mut BTreeSet<String>,
    non_literal_kinds: &mut Vec<(PathBuf, String)>,
) {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    // Necessary-condition pre-filter. `CacheKindCollector` only acts on
    // `ExprMethodCall`s whose method is `insert_arc_with_kind`; every observed
    // cache-kind literal (and every non-literal-kind violation) originates from
    // such a call. A file whose text does NOT contain the `insert_arc_with_kind`
    // identifier cannot contain that method call, so it can contribute neither an
    // observed kind nor a non-literal violation — skipping its `syn::parse_file`
    // is coverage-safe and cannot hide a `missing`/`unexpected`/non-literal
    // result the unfiltered scan would have produced.
    if !src.contains("insert_arc_with_kind") {
        return;
    }
    let parsed =
        syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let mut collector = CacheKindCollector::new(path.to_path_buf(), kinds, non_literal_kinds);
    collector.visit_file(&parsed);
}

/// Group violations by `(file_suffix, fn_name)` using the suffix
/// extraction rule used by the allow-list match (path normalised
/// to forward slashes, suffix is `ends_with` against each allow-
/// list entry's first tuple element). Returns a map keyed by
/// `(file_suffix_or_full_path, fn_name)` to the observed loose-
/// admission call count.
///
/// A violation whose full path does NOT end with any documented
/// allow-list suffix gets keyed by its full normalised path (so
/// the expected-vs-observed diff surfaces "new file/fn pair"
/// failures explicitly).
fn group_violations_by_allow_list_key(
    violations: &[Violation],
    expected: &[(&'static str, &'static str, usize)],
) -> BTreeMap<(String, String), usize> {
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for v in violations {
        let p = v.file.to_string_lossy().replace('\\', "/");
        let matched_suffix = expected
            .iter()
            .find(|(suffix, fn_name, _)| p.ends_with(*suffix) && v.enclosing_fn == *fn_name)
            .map(|(suffix, _, _)| (*suffix).to_string())
            .unwrap_or_else(|| p.clone());
        *counts
            .entry((matched_suffix, v.enclosing_fn.clone()))
            .or_insert(0) += 1;
    }
    counts
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
        "found {} loose-admission call site(s):\n{}",
        violations.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Production-tree guard.
// ---------------------------------------------------------------------------

/// Fact-validated caches in `crates/verter_session/src/**/*.rs` admit
/// through `insert_arc_with_kind` only, EXCEPT for the documented
/// allow-list in `EXPECTED_LOOSE_ADMISSION_COUNTS`. The check is
/// EXACT — the observed loose-admission call count per
/// `(file_suffix, fn)` pair must equal the expected count. Any
/// deviation (surplus, deficit, or brand-new pair) fails the test.
#[test]
fn fact_validated_caches_use_strict_admission_in_production_source() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }

    let observed = group_violations_by_allow_list_key(&violations, EXPECTED_LOOSE_ADMISSION_COUNTS);

    let mut expected: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (suffix, fn_name, count) in EXPECTED_LOOSE_ADMISSION_COUNTS {
        expected.insert(((*suffix).to_string(), (*fn_name).to_string()), *count);
    }

    let mut errors: Vec<String> = Vec::new();

    // Surplus / mismatch / unknown.
    for ((key_suffix, fn_name), observed_count) in &observed {
        match expected.get(&(key_suffix.clone(), fn_name.clone())) {
            Some(expected_count) if expected_count == observed_count => {}
            Some(expected_count) => {
                errors.push(format!(
                    "loose-admission count drift in `{key_suffix}::{fn_name}`: expected \
                     {expected_count}, observed {observed_count}. If this is intentional, \
                     update `EXPECTED_LOOSE_ADMISSION_COUNTS`."
                ));
            }
            None => {
                errors.push(format!(
                    "unexpected loose-admission call site at `{key_suffix}::{fn_name}` \
                     (count: {observed_count}). Either rewrite the call to \
                     `<cache>.insert_arc_with_kind(...)` OR extend \
                     `EXPECTED_LOOSE_ADMISSION_COUNTS` with the new \
                     `(file_suffix, fn, count)` entry after recording the audit \
                     justification."
                ));
            }
        }
    }

    // Deficit: a documented entry has zero observed calls.
    for (key, expected_count) in &expected {
        let observed_count = observed.get(key).copied().unwrap_or(0);
        if observed_count == 0 && *expected_count > 0 {
            errors.push(format!(
                "documented loose-admission call site `{}::{}` not observed in source \
                 (expected count: {}, observed: 0). Either the call site was migrated \
                 to `insert_arc_with_kind` (drop the entry from \
                 `EXPECTED_LOOSE_ADMISSION_COUNTS`) or the function was renamed \
                 (update the list).",
                key.0, key.1, expected_count
            ));
        }
    }

    assert!(
        errors.is_empty(),
        "fact-validated caches must admit through `insert_arc_with_kind` \
         (not `insert` / `insert_arc`). The strict path enforces R20 \
         empty-signature refusal.\n\n\
         Drift detected ({} issue(s)):\n  - {}\n\n\
         Observed loose-admission counts:\n{}\n\n\
         Raw violations:\n{}",
        errors.len(),
        errors.join("\n  - "),
        format_observed(&observed),
        format_violations(&violations)
    );
}

fn format_observed(observed: &BTreeMap<(String, String), usize>) -> String {
    if observed.is_empty() {
        return "  <none>".to_string();
    }
    observed
        .iter()
        .map(|((suffix, fn_name), count)| format!("  {suffix}::{fn_name} = {count}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pin the production-source set of cache-kind literals passed to
/// `insert_arc_with_kind(...)` to `PRODUCTION_CACHE_KINDS`. Both
/// directions are asserted: every expected kind must appear in
/// source (a renamed/deleted producer fails this) AND every
/// observed kind must be in the expected list (a new producer
/// added without updating the list fails this). The same list
/// lives in `r20_admission_refuses_empty_signature.rs` — keep
/// the two in lockstep.
#[test]
fn every_production_cache_kind_is_present_in_source() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut observed_kinds: BTreeSet<String> = BTreeSet::new();
    let mut non_literal_kinds: Vec<(PathBuf, String)> = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file_for_cache_kinds(&file, &mut observed_kinds, &mut non_literal_kinds);
    }

    let expected_kinds: BTreeSet<String> = PRODUCTION_CACHE_KINDS
        .iter()
        .map(|s| (*s).to_string())
        .collect();

    let missing: Vec<&String> = expected_kinds.difference(&observed_kinds).collect();
    let unexpected: Vec<&String> = observed_kinds.difference(&expected_kinds).collect();

    assert!(
        missing.is_empty() && unexpected.is_empty() && non_literal_kinds.is_empty(),
        "production cache-kind set drifted from `PRODUCTION_CACHE_KINDS`:\n\
         \n  Missing kinds (declared in PRODUCTION_CACHE_KINDS but no producer found):\n    {}\n\
         \n  Unexpected kinds (producer exists but kind not in PRODUCTION_CACHE_KINDS):\n    {}\n\
         \n  Non-literal cache_kind arguments (must be a `&'static str` literal):\n    {}\n\
         \n  Observed set:\n    {}\n\
         \n  Expected set:\n    {}\n\
         \nIf you added a new fact-validated cache, update BOTH \
         `insert_arc_strict_admission_required.rs::PRODUCTION_CACHE_KINDS` \
         AND `r20_admission_refuses_empty_signature.rs::PRODUCTION_CACHE_KINDS`.",
        if missing.is_empty() {
            "<none>".to_string()
        } else {
            missing
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ")
        },
        if unexpected.is_empty() {
            "<none>".to_string()
        } else {
            unexpected
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ")
        },
        if non_literal_kinds.is_empty() {
            "<none>".to_string()
        } else {
            non_literal_kinds
                .iter()
                .map(|(p, desc)| format!("{}: {desc}", p.display()))
                .collect::<Vec<_>>()
                .join("\n    ")
        },
        observed_kinds
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", "),
        expected_kinds
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", "),
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
    // Fixture A: `self.cache.insert(...)` from an unrelated fn — REJECTED.
    let fixture_a = r#"
        fn arbitrary_caller(this: &Foo) {
            this.cache.insert(k, v, facts);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_a).is_empty(),
        "scanner failed to flag `self.cache.insert(...)` in arbitrary fn"
    );

    // Fixture B: `self.roots.insert_arc(...)` from an unrelated fn — REJECTED.
    let fixture_b = r#"
        fn arbitrary_caller(this: &Foo) {
            this.roots.insert_arc(k, v, facts);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_b).is_empty(),
        "scanner failed to flag `self.roots.insert_arc(...)`"
    );

    // Fixture C: `self.routes.insert_arc_with_kind(...)` — ACCEPTED.
    let fixture_c = r#"
        fn arbitrary_caller(this: &Foo) {
            this.routes.insert_arc_with_kind(k, v, facts, "route_db.routes");
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_c).is_empty(),
        "scanner incorrectly flagged `insert_arc_with_kind`: {:?}",
        scan_fixture_violations(fixture_c)
    );

    // Fixture D: `vec.insert(...)` (unrelated `Vec::insert`) — ACCEPTED.
    let fixture_d = r#"
        fn arbitrary_caller(v: &mut Vec<u32>) {
            v.insert(0, 42);
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_d).is_empty(),
        "scanner incorrectly flagged `Vec::insert`: {:?}",
        scan_fixture_violations(fixture_d)
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
        scan_fixture_violations(fixture_e).is_empty(),
        "scanner incorrectly flagged unrelated map insert: {:?}",
        scan_fixture_violations(fixture_e)
    );

    // Fixture F: a call inside an allow-listed fn name is STILL
    // recorded by the scanner — the allow-list is now enforced by
    // the production-tree assertion (an exact `(file_suffix, fn) ->
    // count` parity check), not by an in-scanner filter. Synthetic
    // fixtures cannot satisfy the file-suffix part of the parity
    // check, so the scanner records the violation; the parity
    // assertion is what reconciles it on the production tree.
    let fixture_f = r#"
        fn store_node(this: &Foo) {
            this.cache.insert(k, v, facts);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_f).is_empty(),
        "scanner should record the call regardless of fn name — \
         allow-listing happens at the production-tree assertion layer"
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
        scan_fixture_violations(fixture_g).is_empty(),
        "scanner incorrectly flagged `#[cfg(test)] mod tests` body: {:?}",
        scan_fixture_violations(fixture_g)
    );
}

/// The production-tree parity check distinguishes three failure
/// modes: surplus (more loose calls than expected), deficit (fewer
/// loose calls than expected), and unknown-pair (a `(file_suffix,
/// fn)` not in the documented list). Each must fail loudly.
#[test]
fn expected_loose_admission_counts_parity_check_discriminates() {
    // Pretend the expected list says there's exactly one loose call
    // in `(some/file.rs, fn_a)`.
    let expected: &[(&str, &str, usize)] = &[("some/file.rs", "fn_a", 1)];

    // Case 1: observed matches exactly — no errors.
    {
        let violations = vec![Violation {
            file: PathBuf::from("/abs/some/file.rs"),
            enclosing_fn: "fn_a".to_string(),
            method: "insert".to_string(),
        }];
        let observed = group_violations_by_allow_list_key(&violations, expected);
        assert_eq!(observed.len(), 1);
        assert_eq!(
            observed.get(&("some/file.rs".to_string(), "fn_a".to_string())),
            Some(&1)
        );
    }

    // Case 2: surplus — observed > expected for a documented entry.
    {
        let violations = vec![
            Violation {
                file: PathBuf::from("/abs/some/file.rs"),
                enclosing_fn: "fn_a".to_string(),
                method: "insert".to_string(),
            },
            Violation {
                file: PathBuf::from("/abs/some/file.rs"),
                enclosing_fn: "fn_a".to_string(),
                method: "insert_arc".to_string(),
            },
        ];
        let observed = group_violations_by_allow_list_key(&violations, expected);
        assert_eq!(
            observed.get(&("some/file.rs".to_string(), "fn_a".to_string())),
            Some(&2),
            "two loose-admission calls inside an allow-listed fn must be \
             surfaced — pre-fix the test only checked file+fn membership"
        );
    }

    // Case 3: unknown-pair — observed key is NOT in the documented list.
    {
        let violations = vec![Violation {
            file: PathBuf::from("/abs/some/file.rs"),
            enclosing_fn: "fn_unknown".to_string(),
            method: "insert".to_string(),
        }];
        let observed = group_violations_by_allow_list_key(&violations, expected);
        // Unknown pair is keyed by the full normalised path
        // because the suffix match failed.
        let key = observed.keys().next().expect("one observation");
        assert!(
            !key.0.ends_with("some/file.rs") || key.1 != "fn_a",
            "unknown-pair key must be distinguishable from the \
             documented `(some/file.rs, fn_a)` entry"
        );
        assert_eq!(
            key.1, "fn_unknown",
            "unknown-pair must preserve the observed fn name"
        );
    }

    // Case 4: a documented entry that surfaces with zero observed
    // calls means a producer was migrated to `insert_arc_with_kind`
    // OR the function was renamed — both must fail the production
    // assertion. Group output for the documented key is missing,
    // and the deficit branch fires.
    {
        let violations: Vec<Violation> = vec![];
        let observed = group_violations_by_allow_list_key(&violations, expected);
        assert!(
            !observed.contains_key(&("some/file.rs".to_string(), "fn_a".to_string())),
            "deficit case: documented entry absent from observed map — \
             the production assertion catches this in its second loop"
        );
    }
}

fn scan_fixture_violations(src: &str) -> Vec<Violation> {
    let parsed = syn::parse_file(src).expect("parse fixture");
    let mut violations = Vec::new();
    let fake_path = Path::new("<fixture>");
    let mut scanner = Scanner::new(fake_path, &mut violations);
    scanner.visit_file(&parsed);
    violations
}
