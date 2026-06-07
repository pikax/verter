//! Architecture guard — every production cold-compute path that
//! publishes into a fact-validated cache must wrap the cold body
//! with `install_fact_tracer` AND admit via `insert_arc_with_kind`.
//!
//! ## Why
//!
//! The fact-signature substrate establishes a two-step contract
//! for every cache entry that carries a `fact_dep_signature`:
//!
//! 1. **Tracer install.** The cold-compute body runs inside
//!    `install_fact_tracer(host, || { ... })`. The helper installs
//!    a fresh `FactReadSetCell` on TLS, runs the body, finalises
//!    the read set, and on `FactReadSetFinalise::Overflow` emits a
//!    `FactSignatureOverflow` event and increments
//!    the per-host `signature_overflow_at_install` counter.
//!    Without the install scope,
//!    the cold body's per-fact `observe_fan_out_borrowed` calls
//!    fan into NOTHING — the producer publishes a value with an
//!    empty (or worse, an outer scope's) read-set signature.
//!
//! 2. **Strict admission.** The publish call uses
//!    `ValidatedFactCache::insert_arc_with_kind(key, value, facts,
//!    cache_kind)`. The strict path refuses empty signatures with
//!    a structured `FactSignatureAdmissionRefused { cache_kind: ... }`
//!    event so a producer that accidentally short-circuited its
//!    tracer never poisons the cache with a phantom-fact entry.
//!
//! Together the two steps ensure the cache's
//! `fact_dep_signature` is the path-precise read set the cold
//! compute observed — every cross-file read is recorded, every
//! later edit to any touched file invalidates the entry.
//!
//! ## What this guard does
//!
//! Scans `crates/verter_session/src/**/*.rs` (excluding sibling
//! `*_tests.rs`, `tests/`, `benches/`, `examples/`, and
//! `#[cfg(test)]` items) for the structural pattern:
//!
//! - Any call to `install_fact_tracer(host, closure)` is recorded.
//! - The call must be paired with a downstream
//!   `insert_arc_with_kind(...)` on a `ValidatedFactCache`-shaped
//!   receiver (in the same enclosing function, OR in a helper the
//!   enclosing function calls — the guard is structural and
//!   asserts the pairing exists in the enclosing function body).
//!
//! ## What this guard catches
//!
//! - A future cold-compute call site that adds an `insert_arc`
//!   (loose admission) inside an `install_fact_tracer` scope — the
//!   bare `insert_arc` writer is caught by
//!   `insert_arc_strict_admission_required.rs`; this guard cross-
//!   checks that the tracer-wrapping wasn't pointless.
//! - A `with_fact_tracer` cold body that does NOT route through
//!   `install_fact_tracer` (a regression that would skip the
//!   overflow event emission). The brief's escalation rule says
//!   `with_fact_tracer` directly is permitted for caches that
//!   publish into bespoke `DashMap`-backed caches (e.g.
//!   `ComponentMetaResultDb`) — those don't admit into a
//!   `ValidatedFactCache` so `insert_arc_with_kind` is N/A. The
//!   guard only flags direct `with_fact_tracer` calls inside fns
//!   that ALSO call `insert_arc_with_kind` on a known
//!   `ValidatedFactCache` field, which would be the wrong pairing.
//!
//! ## Allow-list
//!
//! The `install_fact_tracer` symbol is defined in
//! `fact_signature_helpers.rs` — its own implementation body
//! legitimately calls `with_fact_tracer`. The scanner exempts that
//! enclosing fn via the helper-definition allow-list.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, ExprCall, ExprMethodCall, ImplItemFn, ItemFn, ItemImpl, ItemMod, Meta};
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

/// Functions whose bodies legitimately call `host.with_fact_tracer`
/// directly (bypassing `install_fact_tracer`). Two categories:
///
/// 1. The helper definition itself (`install_fact_tracer` in
///    `fact_signature_helpers.rs`).
/// 2. Bespoke-cache cold compute paths that publish into a
///    `DashMap`-backed cache (NOT a `ValidatedFactCache`). These
///    sites construct their `read_set_signature` from
///    `resolved.fact_versions` rather than from the tracer's
///    finalised read set directly, so the tracer scope is purely a
///    fan-out conduit, not a signature source. The bespoke caches
///    have their own admission path (`.insert(...)`).
const DIRECT_WITH_FACT_TRACER_ALLOW: &[(&str, &str)] = &[
    // Helper definition.
    ("fact_signature_helpers.rs", "install_fact_tracer"),
    // Bespoke-cache (DashMap-backed) cold computes. These admit
    // into `ComponentMetaResultDb` / similar via
    // `publish_component_meta_cache_entry*`, NOT into a
    // `ValidatedFactCache`.
    (
        "host_manage/component_meta_entry.rs",
        "get_or_resolve_component_meta",
    ),
    (
        "host_manage/component_meta_entry.rs",
        "get_or_resolve_component_meta_with_view",
    ),
    (
        "host_manage/component_meta_entry.rs",
        "get_or_resolve_component_meta_resolved_analysis",
    ),
    // Virtual-file compile pipeline — admits into the compile-fact
    // bus, not a `ValidatedFactCache`.
    (
        "host_resolve/virtual_file_pipeline.rs",
        "compile_to_outputs_using_audit_runtime",
    ),
];

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    enclosing_fn: String,
    kind: ViolationKind,
}

#[derive(Debug)]
enum ViolationKind {
    /// `host.with_fact_tracer(...)` invoked directly from a fn
    /// that ALSO writes through `<cache>.insert_arc_with_kind(...)`,
    /// where `<cache>` is a `ValidatedFactCache`-shaped field. The
    /// production pattern is `install_fact_tracer` for that pairing.
    DirectWithFactTracerWithStrictAdmission,
    /// `install_fact_tracer(...)` scope present but admission goes
    /// through the loose `<cache>.insert_arc(...)` path. The strict
    /// admission gate (`insert_arc_with_kind`) is the production
    /// pattern. This is the dual to
    /// `insert_arc_strict_admission_required.rs` — the strict-admission
    /// scanner catches the loose-write site itself; this guard
    /// catches the tracer-was-installed-but-strict-admission-skipped
    /// pairing.
    InstallFactTracerWithLooseAdmission,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let detail = match self.kind {
            ViolationKind::DirectWithFactTracerWithStrictAdmission => {
                "direct `with_fact_tracer` call paired with `insert_arc_with_kind` \
                 — use `install_fact_tracer` so the overflow event fires"
            }
            ViolationKind::InstallFactTracerWithLooseAdmission => {
                "`install_fact_tracer` scope paired with a loose `insert_arc(...)` admission \
                 — switch the writer to `insert_arc_with_kind` so R20 refuses empty signatures"
            }
        };
        write!(
            f,
            "{}: in fn `{}`: {}",
            self.file.display(),
            self.enclosing_fn,
            detail
        )
    }
}

/// Per-function scratch state used to detect the pairing of
/// `install_fact_tracer` / `with_fact_tracer` against
/// `insert_arc` / `insert_arc_with_kind` within the same enclosing
/// fn body.
#[derive(Default)]
struct FnPairings {
    saw_install_fact_tracer: bool,
    saw_direct_with_fact_tracer: bool,
    saw_insert_arc_loose_on_known_cache: bool,
    saw_insert_arc_with_kind_on_known_cache: bool,
}

struct Scanner<'a> {
    file: &'a Path,
    fn_stack: Vec<(String, FnPairings)>,
    cfg_test_depth: u32,
    violations: &'a mut Vec<Violation>,
    direct_with_fact_tracer_allow: HashSet<(&'static str, &'static str)>,
}

impl<'a> Scanner<'a> {
    fn new(file: &'a Path, violations: &'a mut Vec<Violation>) -> Self {
        Self {
            file,
            fn_stack: Vec::new(),
            cfg_test_depth: 0,
            violations,
            direct_with_fact_tracer_allow: DIRECT_WITH_FACT_TRACER_ALLOW.iter().copied().collect(),
        }
    }

    fn current_pairings(&mut self) -> Option<&mut FnPairings> {
        self.fn_stack.last_mut().map(|(_, p)| p)
    }

    fn is_direct_with_fact_tracer_allowed(&self, file: &Path, enclosing_fn: &str) -> bool {
        let p = file.to_string_lossy().replace('\\', "/");
        self.direct_with_fact_tracer_allow
            .iter()
            .any(|(suffix, fn_name)| p.ends_with(*suffix) && enclosing_fn == *fn_name)
    }

    fn finalise_fn(&mut self) {
        if self.cfg_test_depth > 0 {
            return;
        }
        let Some((fn_name, pairings)) = self.fn_stack.last() else {
            return;
        };
        let fn_name = fn_name.clone();

        // Detection 1: direct `with_fact_tracer` + strict admission
        // in the same fn body. This is the unmigrated-pairing pattern.
        // Allow-listed call sites bypass.
        if pairings.saw_direct_with_fact_tracer
            && pairings.saw_insert_arc_with_kind_on_known_cache
            && !self.is_direct_with_fact_tracer_allowed(self.file, &fn_name)
        {
            self.violations.push(Violation {
                file: self.file.to_path_buf(),
                enclosing_fn: fn_name.clone(),
                kind: ViolationKind::DirectWithFactTracerWithStrictAdmission,
            });
        }

        // Detection 2: `install_fact_tracer` present + loose
        // `insert_arc` admission in the same fn body. The strict
        // admission path is the production correctness gate;
        // skipping it under an installed tracer scope is the
        // exact "empty-signature on warm write" regression the
        // guard is named for.
        if pairings.saw_install_fact_tracer && pairings.saw_insert_arc_loose_on_known_cache {
            self.violations.push(Violation {
                file: self.file.to_path_buf(),
                enclosing_fn: fn_name,
                kind: ViolationKind::InstallFactTracerWithLooseAdmission,
            });
        }
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
        self.fn_stack
            .push((f.sig.ident.to_string(), FnPairings::default()));
        syn::visit::visit_item_fn(self, f);
        self.finalise_fn();
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
        self.fn_stack
            .push((f.sig.ident.to_string(), FnPairings::default()));
        syn::visit::visit_impl_item_fn(self, f);
        self.finalise_fn();
        self.fn_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_expr_call(&mut self, c: &'ast ExprCall) {
        // `install_fact_tracer(host, closure)` — free-function call.
        if let syn::Expr::Path(p) = &*c.func {
            if let Some(last_seg) = p.path.segments.last() {
                if last_seg.ident == "install_fact_tracer" {
                    if let Some(pairings) = self.current_pairings() {
                        pairings.saw_install_fact_tracer = true;
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, c);
    }

    fn visit_expr_method_call(&mut self, c: &'ast ExprMethodCall) {
        let method = c.method.to_string();
        match method.as_str() {
            // `host.with_fact_tracer(closure)` — method on
            // `VerterHost`. We only count it as "direct" when not
            // wrapped by an `install_fact_tracer` call inside the
            // same fn body. Since the visitor walks linearly, we
            // record both signals and decide at fn-exit.
            "with_fact_tracer" => {
                if let Some(pairings) = self.current_pairings() {
                    pairings.saw_direct_with_fact_tracer = true;
                }
            }
            "insert_arc" => {
                if receiver_is_known_validated_fact_cache_field(&c.receiver) {
                    if let Some(pairings) = self.current_pairings() {
                        pairings.saw_insert_arc_loose_on_known_cache = true;
                    }
                }
            }
            "insert_arc_with_kind" => {
                if receiver_is_known_validated_fact_cache_field(&c.receiver) {
                    if let Some(pairings) = self.current_pairings() {
                        pairings.saw_insert_arc_with_kind_on_known_cache = true;
                    }
                }
            }
            _ => {}
        }
        syn::visit::visit_expr_method_call(self, c);
    }
}

const VALIDATED_FACT_CACHE_FIELDS: &[&str] = &[
    "cache",
    "roots",
    "routes",
    "barrel_surfaces",
    "effective_export_sets",
    "prepared_decl_bundles",
    "component_meta",
];

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
    // Necessary-condition pre-filter: BOTH violation kinds require a
    // tracer call AND a `ValidatedFactCache` admission in the same fn
    // body. The tracer call is `install_fact_tracer` or
    // `with_fact_tracer` (both contain `fact_tracer`); the admission is
    // `insert_arc` or `insert_arc_with_kind` (both contain `insert_arc`).
    // A file missing either substring cannot produce a pairing, so skip
    // the `syn` parse + AST walk. Both substrings are strict
    // prerequisites for the pairings this guard detects, so filtering
    // cannot hide a violation.
    if !(src.contains("fact_tracer") && src.contains("insert_arc")) {
        return;
    }
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
            lines.push(format!("    fn `{}`: {:?}", v.enclosing_fn, v.kind));
        }
    }
    format!(
        "found {} fact-signature pairing violation(s):\n{}",
        violations.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Production-tree guard.
// ---------------------------------------------------------------------------

/// Every production cold-compute fn that pairs an installed tracer
/// scope with an admission into a `ValidatedFactCache`-shaped
/// receiver must use BOTH `install_fact_tracer` (for the tracer
/// install) AND `insert_arc_with_kind` (for the admission).
/// Pre-existing direct `with_fact_tracer` call sites that publish
/// into bespoke (DashMap-backed) caches are allow-listed.
#[test]
fn cold_compute_paths_use_install_fact_tracer_and_strict_admission() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "cold-compute pairings must use BOTH `install_fact_tracer` AND \
         `insert_arc_with_kind`. The strict-admission path enforces R20 \
         empty-signature refusal; `install_fact_tracer` ensures the \
         overflow event fires on FactReadSetFinalise::Overflow. Allow-listed \
         direct `with_fact_tracer` call sites (bespoke DashMap caches):\n  - {}\n\n\
         Fix: route the cold body through `install_fact_tracer(host, || {{ ... }})` \
         and admit via `<cache>.insert_arc_with_kind(key, value, facts, \"<kind>\")`.\n\n{}",
        DIRECT_WITH_FACT_TRACER_ALLOW
            .iter()
            .map(|(f, fn_name)| format!("{f}::{fn_name}"))
            .collect::<Vec<_>>()
            .join("\n  - "),
        format_violations(&violations)
    );
}

// ---------------------------------------------------------------------------
// Sentinel: discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Drive the scanner against synthetic source to confirm the
/// classification works. Without these fixtures, the production
/// guard could pass trivially.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: `install_fact_tracer` paired with
    // `insert_arc_with_kind` on `self.routes` — ACCEPTED.
    let fixture_a = r#"
        fn cold_compute(this: &Foo, host: &VerterHost) {
            let (_, finalise) = install_fact_tracer(host, || {
                /* cold body */
            });
            this.routes.insert_arc_with_kind(k, v, facts, "route_db.routes");
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_a).is_empty(),
        "scanner incorrectly flagged the install-then-strict-admit pattern: {:?}",
        scan_fixture_violations(fixture_a)
    );

    // Fixture B: direct `host.with_fact_tracer` + strict admission
    // on `self.routes` — REJECTED (the production pattern requires
    // `install_fact_tracer`).
    let fixture_b = r#"
        fn cold_compute_skipping_install(this: &Foo, host: &VerterHost) {
            let (_, read_set) = host.with_fact_tracer(|| {
                /* cold body */
            });
            this.routes.insert_arc_with_kind(k, v, facts, "route_db.routes");
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_b).is_empty(),
        "scanner failed to flag direct `with_fact_tracer` + strict admission"
    );

    // Fixture C: `install_fact_tracer` paired with loose
    // `insert_arc` on `self.routes` — REJECTED (the strict gate
    // was skipped at the admission site).
    let fixture_c = r#"
        fn cold_compute_loose_admit(this: &Foo, host: &VerterHost) {
            let (_, finalise) = install_fact_tracer(host, || {
                /* cold body */
            });
            this.routes.insert_arc(k, v, facts);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_c).is_empty(),
        "scanner failed to flag install-then-loose-admit"
    );

    // Fixture D: direct `with_fact_tracer` WITHOUT any
    // `ValidatedFactCache` admission — ACCEPTED. The bespoke cache
    // case (compile-fact, ComponentMetaResultDb) admits through
    // `.insert(...)` on its own non-`ValidatedFactCache` substrate.
    let fixture_d = r#"
        fn cold_compute_bespoke(this: &Foo, host: &VerterHost) {
            let (result, read_set) = host.with_fact_tracer(|| {
                /* cold body */
            });
            this.bespoke_db.publish(result);
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_d).is_empty(),
        "scanner incorrectly flagged direct `with_fact_tracer` without strict admission: {:?}",
        scan_fixture_violations(fixture_d)
    );

    // Fixture E: `install_fact_tracer` paired with
    // `insert_arc_with_kind` on a NON-listed field (the receiver
    // is not a known `ValidatedFactCache` field) — ACCEPTED. The
    // guard's known-field list is intentionally narrow to avoid
    // false-positives on unrelated method calls.
    let fixture_e = r#"
        fn cold_compute_unrelated_cache(this: &Foo, host: &VerterHost) {
            let (_, finalise) = install_fact_tracer(host, || {
                /* cold body */
            });
            this.unrelated_field.insert(k, v);
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_e).is_empty(),
        "scanner incorrectly flagged unrelated cache admission: {:?}",
        scan_fixture_violations(fixture_e)
    );

    // Fixture F: `#[cfg(test)] mod tests` body — ACCEPTED. Test
    // scaffolding is exempt.
    let fixture_f = r#"
        #[cfg(test)]
        mod tests {
            fn helper(this: &Foo, host: &VerterHost) {
                let (_, _) = host.with_fact_tracer(|| {});
                this.routes.insert_arc_with_kind(k, v, facts, "route_db.routes");
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_f).is_empty(),
        "scanner incorrectly flagged `#[cfg(test)] mod tests` body: {:?}",
        scan_fixture_violations(fixture_f)
    );

    // Fixture G: nested fns / impls — direct `with_fact_tracer`
    // in an inner fn that does NOT itself admit. Detection is
    // per-fn-body, so the outer fn's admission does not pair with
    // the inner fn's tracer call — ACCEPTED.
    let fixture_g = r#"
        fn outer(this: &Foo, host: &VerterHost) {
            fn inner(host: &VerterHost) {
                let (_, _) = host.with_fact_tracer(|| {});
            }
            inner(host);
            this.routes.insert_arc_with_kind(k, v, facts, "route_db.routes");
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_g).is_empty(),
        "scanner incorrectly flagged nested-fn tracer call: {:?}",
        scan_fixture_violations(fixture_g)
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
