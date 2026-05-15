//! Architecture guard — Block 6.A: Family B/C/D consumer migration
//! completeness audit.
//!
//! Blocks 1.H/1.I migrated every production-source read of the
//! legacy carrier field `entry.dep_signature` onto the
//! carrier-style access `entry.read_set_signature.legacy`. Block 6.B
//! will retire the carrier's `legacy: DepSignature` field entirely.
//!
//! Block 6.A's narrower task is to ASSERT, by scanning the production
//! source of `crates/verter_session/src/`, that zero direct
//! `entry.dep_signature` field reads remain — only test fixtures may
//! still reference the legacy field name. If any production-tree
//! match is found, Block 1.H/1.I left work unfinished and the next
//! migration block cannot land safely.
//!
//! Scope:
//!
//! - File walk: `crates/verter_session/src/**/*.rs` excluding
//!   `tests/`, `benches/`, `examples/`, and any sibling `*_tests.rs`
//!   / `tests.rs` files. Test scaffolding is exempt because
//!   characterization tests may legitimately reference the legacy
//!   field.
//! - In-source carve-outs: `#[cfg(test)] mod ...`, `mod tests`, and
//!   `#[cfg(test)] impl` blocks. The visitor maintains a
//!   `cfg_test_depth` counter and suppresses violations recorded
//!   inside any test-only region.
//! - Pattern: an `ExprField` whose base resolves to the bare path
//!   `entry` (single segment, no leading `::`, no generics) AND whose
//!   `member` is the named field `dep_signature`. This matches
//!   `entry.dep_signature` exactly and DOES NOT match
//!   `entry.read_set_signature.legacy` (chained field access whose
//!   outer base is itself an `ExprField`, not a path).
//!
//! Allow-list:
//!
//! - Production-source allow-list: EMPTY. There is no production
//!   reader of `entry.dep_signature` after Blocks 1.H/1.I. The guard
//!   trips on every match.
//!
//! Trigger conditions:
//!
//! - Re-introducing `entry.dep_signature` in any production source
//!   file under `crates/verter_session/src/` FAILS the
//!   `entry_dep_signature_field_reads_eliminated_from_production_tree`
//!   test.
//! - The visitor's discriminating-property fixtures double-check
//!   that the scanner ACTUALLY classifies the pattern (a stub-
//!   prevention measure — without these, the production guard could
//!   pass trivially if the scanner failed to detect anything).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, Expr, ExprField, ImplItemFn, ItemFn, ItemImpl, ItemMod, Member, Meta};
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
            "{}: in fn `{}`: `{}` -- legacy carrier field reads must \
             route through `entry.read_set_signature.legacy`",
            self.file.display(),
            self.enclosing_fn,
            self.callsite
        )
    }
}

/// Visitor that records every `ExprField` whose base resolves to the
/// bare path `entry` and whose member is the named field
/// `dep_signature`. The visitor walks only production-source items;
/// `#[cfg(test)] mod tests`, `mod tests`, and `#[cfg(test)] impl`
/// blocks are skipped so test fixtures (which may legitimately
/// reference the legacy field) are not flagged.
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

    fn record(&mut self, callsite: String) {
        if self.cfg_test_depth > 0 {
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
        // carrier field without tripping the guard.
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

    fn visit_expr_field(&mut self, e: &'ast ExprField) {
        // Match `<expr>.dep_signature` where `<expr>` is the bare
        // local binding `entry`. We reject:
        //
        //   entry.dep_signature
        //
        // and accept (do not match) the carrier-style access:
        //
        //   entry.read_set_signature.legacy
        //
        // because the outer `ExprField` for `.legacy` has a base that
        // is itself an `ExprField`, not an `ExprPath`. We also accept
        // chained reads through other bindings (`record.dep_signature`,
        // `state.dep_signature`, etc.) because Block 6.A's contract is
        // narrowed to the `entry` binding used in carrier-aware
        // closures.
        if is_named_field(&e.member, "dep_signature") && is_bare_path(&e.base, "entry") {
            self.record(format!("entry.{}", member_name(&e.member)));
        }
        syn::visit::visit_expr_field(self, e);
    }
}

fn is_named_field(m: &Member, name: &str) -> bool {
    match m {
        Member::Named(ident) => ident == name,
        Member::Unnamed(_) => false,
    }
}

fn member_name(m: &Member) -> String {
    match m {
        Member::Named(ident) => ident.to_string(),
        Member::Unnamed(idx) => format!("{}", idx.index),
    }
}

/// True when `expr` is the bare path `name` — a single path segment,
/// no leading `::`, no generic arguments, no `super::`/`crate::`
/// prefix. This is what `|entry: &Entry| { entry.dep_signature }`
/// produces for the `entry` binding inside a closure body.
fn is_bare_path(expr: &Expr, name: &str) -> bool {
    let path = match expr {
        Expr::Path(p) => &p.path,
        _ => return false,
    };
    if path.leading_colon.is_some() {
        return false;
    }
    if path.segments.len() != 1 {
        return false;
    }
    let seg = &path.segments[0];
    if !matches!(seg.arguments, syn::PathArguments::None) {
        return false;
    }
    seg.ident == name
}

/// `#[cfg(test)]`, `#[cfg(any(test, ...))]`, or `#[cfg(all(..., test, ...))]`.
/// Mirrors the helper in
/// `no_accumulate_dispatch_dep_signature_outside_helpers.rs` so
/// cfg-test items are uniformly recognised as test-only.
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
        "found {} legacy `entry.dep_signature` field-read violation(s):\n{}",
        violations.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Production-tree guard.
// ---------------------------------------------------------------------------

/// Block 1.H/1.I migrated every production-source carrier read away
/// from `entry.dep_signature` onto the carrier-style accessor
/// `entry.read_set_signature.legacy`. This test asserts the
/// migration is complete by walking `crates/verter_session/src/` and
/// requiring ZERO direct `entry.dep_signature` reads outside test
/// fixtures.
///
/// If this test fails, Block 6.B (which deletes the `legacy:
/// DepSignature` field from the carrier) cannot land safely. The fix
/// is to rewrite the flagged call site to use
/// `entry.read_set_signature.legacy` (carrier-style access) or, if
/// possible, to drop the legacy whole-hash rail entirely and consume
/// the tracer-authored `entry.read_set_signature.facts` rail.
#[test]
fn entry_dep_signature_field_reads_eliminated_from_production_tree() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "legacy `entry.dep_signature` field reads must be eliminated \
         from production source before Block 6.B can retire the \
         `legacy: DepSignature` carrier field. Rewrite each call site \
         to access `entry.read_set_signature.legacy` (the carrier-style \
         accessor) instead.\n\n{}",
        format_violations(&violations)
    );
}

// ---------------------------------------------------------------------------
// Sentinel: discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Drive the scanner against synthetic source to confirm the
/// classification works. Without this check, the production-tree
/// guard could pass trivially if the scanner never detected ANY
/// `entry.dep_signature` pattern.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: bare `entry.dep_signature` read inside an arbitrary
    // fn — REJECTED. The classic legacy access pattern.
    let fixture_a = r#"
        fn arbitrary_caller() {
            let _ = entry.dep_signature;
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_a).is_empty(),
        "scanner failed to flag bare `entry.dep_signature` read"
    );

    // Fixture B: `Arc::clone(&entry.dep_signature)` — REJECTED.
    // Borrow inside a function call, the exact shape used by the
    // pre-migration carrier readers.
    let fixture_b = r#"
        fn arbitrary_caller() {
            let _ = Arc::clone(&entry.dep_signature);
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_b).is_empty(),
        "scanner failed to flag `Arc::clone(&entry.dep_signature)`"
    );

    // Fixture C: closure body `|entry: &Entry| entry.dep_signature` —
    // REJECTED. The visitor walks closure bodies; the violation is
    // recorded with the enclosing free fn name, not the closure.
    let fixture_c = r#"
        fn arbitrary_caller() {
            let _f = |entry: &Entry| entry.dep_signature;
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_c).is_empty(),
        "scanner failed to flag `|entry| entry.dep_signature` closure body"
    );

    // Fixture D: carrier-style access `entry.read_set_signature.legacy` —
    // ACCEPTED. The outer `ExprField` is `.legacy` whose base is an
    // `ExprField` (`.read_set_signature`), not a bare path; and the
    // inner `.read_set_signature` does not match the watched
    // `dep_signature` member. So neither field access in the chain
    // trips the scanner.
    let fixture_d = r#"
        fn arbitrary_caller() {
            let _ = Arc::clone(&entry.read_set_signature.legacy);
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_d).is_empty(),
        "scanner incorrectly flagged the carrier-style accessor \
         `entry.read_set_signature.legacy`: {:?}",
        scan_fixture_violations(fixture_d)
    );

    // Fixture E: chained field access through a different binding —
    // ACCEPTED. Block 6.A's contract narrows the watched binding to
    // the closure-parameter name `entry`; `record.dep_signature`,
    // `state.dep_signature`, etc. are NOT flagged. (A broader guard
    // would belong in a different block.)
    let fixture_e = r#"
        fn arbitrary_caller() {
            let _ = record.dep_signature;
            let _ = state.dep_signature;
            let _ = self.dep_signature;
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_e).is_empty(),
        "scanner incorrectly flagged chained field access through a \
         non-`entry` binding: {:?}",
        scan_fixture_violations(fixture_e)
    );

    // Fixture F: struct literal field name — ACCEPTED. A field name
    // appearing in a struct-literal position (`StructName {
    // dep_signature: ..., ... }`) parses as `FieldValue`, not
    // `ExprField`. The scanner only walks `ExprField`, so struct
    // initialisers naming `dep_signature` are never flagged.
    let fixture_f = r#"
        fn arbitrary_caller() {
            let _ = SomeStruct {
                dep_signature: Arc::clone(&entry.read_set_signature.legacy),
                walker_diagnostics: Arc::from([]),
            };
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_f).is_empty(),
        "scanner incorrectly flagged a struct-literal field name: {:?}",
        scan_fixture_violations(fixture_f)
    );

    // Fixture G: `#[cfg(test)] mod tests` block with the offending
    // pattern — ACCEPTED. Test scaffolding is exempt from the guard
    // so characterization tests may legitimately exercise the legacy
    // carrier field.
    let fixture_g = r#"
        #[cfg(test)]
        mod tests {
            fn test_helper() {
                let _ = entry.dep_signature;
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_g).is_empty(),
        "scanner incorrectly flagged a `#[cfg(test)] mod tests` body: {:?}",
        scan_fixture_violations(fixture_g)
    );

    // Fixture H: bare `mod tests { ... }` (no `#[cfg(test)]` attr) —
    // ACCEPTED. The visitor also exempts modules named `tests` to
    // mirror the in-source carve-out used by
    // `no_accumulate_dispatch_dep_signature_outside_helpers.rs`.
    let fixture_h = r#"
        mod tests {
            fn helper() {
                let _ = entry.dep_signature;
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_h).is_empty(),
        "scanner incorrectly flagged a `mod tests {{ ... }}` body: {:?}",
        scan_fixture_violations(fixture_h)
    );

    // Fixture I: nested impl-method calling the offending pattern —
    // REJECTED. The scanner walks `ImplItemFn` and applies the same
    // detection rule as for free fns.
    let fixture_i = r#"
        impl Foo {
            fn bar(&self, entry: &Entry) {
                let _ = entry.dep_signature;
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_i).is_empty(),
        "scanner failed to flag impl-method body referencing `entry.dep_signature`"
    );

    // Fixture J: qualified-path base — ACCEPTED. The scanner narrows
    // matches to a BARE `entry` path; `crate::cache::entry.dep_signature`
    // (hypothetical) doesn't apply because the base would not be a
    // single-segment path. This guards against false positives on
    // unrelated module paths whose final segment happens to be
    // `entry`.
    let fixture_j = r#"
        fn arbitrary_caller() {
            let _ = crate::cache::ENTRY.dep_signature;
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_j).is_empty(),
        "scanner incorrectly flagged a qualified-path base whose final \
         segment differs from a bare `entry` binding: {:?}",
        scan_fixture_violations(fixture_j)
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
