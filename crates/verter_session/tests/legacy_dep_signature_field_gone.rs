//! Architecture guard — legacy `DepSignature` field on
//! `ReadSetSignature` (and any other cache-carrier struct) is gone.
//!
//! Cache validity is a single oracle: the path-precise fact-tracer
//! signature `ReadSetSignature.facts: Arc<[FactVersionRef]>`. The
//! bundled whole-hash / project-generation `DepSignature`
//! cache-validity rail — once carried as `ReadSetSignature.legacy` —
//! is retired. No cache-carrier struct may carry a public
//! `DepSignature` field.
//!
//! This guard scans `crates/verter_session/src/**/*.rs` (production
//! source) for any `pub <name>: DepSignature` field declaration
//! inside a struct whose role is "cache carrier" — concretely, any
//! struct named `ReadSetSignature` (the carrier type) or any
//! per-cache entry struct with the suffix `*Entry` / `*CacheEntry` /
//! `*Signature` / `*Snapshot`. Re-introduction of a public legacy
//! `DepSignature` rail is a regression.
//!
//! The dispatch-return signature relocated onto `MemoEntry` as a
//! `pub(super)` field is intentionally NOT in scope: it is not a
//! cache-validity rail, it is the dispatch accumulator's transitive
//! input, and the guard targets the *public* validity rail only (a
//! crate-private field is a separate, far smaller concern — see
//! fixture C).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Field, Fields, ItemStruct, Type, Visibility};
use walkdir::WalkDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    struct_name: String,
    field_name: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: `{}.{}: DepSignature` -- legacy field must be deleted",
            self.file.display(),
            self.struct_name,
            self.field_name
        )
    }
}

/// True if the type expression's last path segment is `DepSignature`.
fn type_is_dep_signature(ty: &Type) -> bool {
    if let Type::Path(tp) = ty {
        return tp
            .path
            .segments
            .last()
            .map(|s| s.ident == "DepSignature")
            .unwrap_or(false);
    }
    false
}

/// True if a struct name marks it as a "cache carrier" — the kind of
/// struct that participates in the fact-based cache validity rail.
fn struct_is_cache_carrier(name: &str) -> bool {
    if name == "ReadSetSignature" {
        return true;
    }
    name.ends_with("Entry")
        || name.ends_with("CacheEntry")
        || name.ends_with("Signature")
        || name.ends_with("Snapshot")
}

struct Scanner<'a> {
    file: &'a Path,
    violations: &'a mut Vec<Violation>,
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_struct(&mut self, s: &'ast ItemStruct) {
        let struct_name = s.ident.to_string();
        if !struct_is_cache_carrier(&struct_name) {
            return;
        }
        if let Fields::Named(named) = &s.fields {
            for field in &named.named {
                if !matches!(field.vis, Visibility::Public(_)) {
                    continue;
                }
                let Field {
                    ident: Some(field_name),
                    ty,
                    ..
                } = field
                else {
                    continue;
                };
                if !type_is_dep_signature(ty) {
                    continue;
                }
                self.violations.push(Violation {
                    file: self.file.to_path_buf(),
                    struct_name: struct_name.clone(),
                    field_name: field_name.to_string(),
                });
            }
        }
    }
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
    let mut scanner = Scanner {
        file: path,
        violations,
    };
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
                "    struct `{}` field `{}: DepSignature`",
                v.struct_name, v.field_name
            ));
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Legacy cache-validity-rail scanner — `validate_dep_signature` symbol
// + `.legacy` field access.
// ---------------------------------------------------------------------------

/// One legacy cache-validity-rail reference in production source.
#[derive(Debug)]
struct RailHit {
    file: PathBuf,
    kind: &'static str,
    detail: String,
}

impl std::fmt::Display for RailHit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: [{}] {}",
            self.file.display(),
            self.kind,
            self.detail
        )
    }
}

/// Scans a production source file for two legacy-rail shapes that the
/// `DepSignature` cache-validity retirement removes:
///
/// 1. The `validate_dep_signature` symbol — the legacy AND-gate
///    validator — as a function/method definition OR a call site.
/// 2. A `.legacy` field access — `<expr>.legacy` — the read of the
///    retired `ReadSetSignature.legacy` cache-validity rail.
///
/// `#[cfg(test)]` regions are skipped so test scaffolding is exempt.
struct RailScanner<'a> {
    file: &'a Path,
    cfg_test_depth: u32,
    hits: &'a mut Vec<RailHit>,
}

impl<'ast> Visit<'ast> for RailScanner<'_> {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        let entered = has_cfg_test_attr(&m.attrs) || m.ident == "tests";
        if entered {
            self.cfg_test_depth += 1;
        }
        syn::visit::visit_item_mod(self, m);
        if entered {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let entered = has_cfg_test_attr(&f.attrs);
        if entered {
            self.cfg_test_depth += 1;
        }
        if self.cfg_test_depth == 0 && f.sig.ident == "validate_dep_signature" {
            self.hits.push(RailHit {
                file: self.file.to_path_buf(),
                kind: "validate_dep_signature-def",
                detail: "`fn validate_dep_signature(...)` -- legacy AND-gate validator".into(),
            });
        }
        syn::visit::visit_item_fn(self, f);
        if entered {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        let entered = has_cfg_test_attr(&f.attrs);
        if entered {
            self.cfg_test_depth += 1;
        }
        if self.cfg_test_depth == 0 && f.sig.ident == "validate_dep_signature" {
            self.hits.push(RailHit {
                file: self.file.to_path_buf(),
                kind: "validate_dep_signature-def",
                detail: "`fn validate_dep_signature(...)` -- legacy AND-gate validator (impl)"
                    .into(),
            });
        }
        syn::visit::visit_impl_item_fn(self, f);
        if entered {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        if self.cfg_test_depth == 0 && f.sig.ident == "validate_dep_signature" {
            self.hits.push(RailHit {
                file: self.file.to_path_buf(),
                kind: "validate_dep_signature-def",
                detail: "`fn validate_dep_signature(...)` -- legacy AND-gate validator (trait)"
                    .into(),
            });
        }
        syn::visit::visit_trait_item_fn(self, f);
    }

    fn visit_expr_method_call(&mut self, c: &'ast syn::ExprMethodCall) {
        if self.cfg_test_depth == 0 && c.method == "validate_dep_signature" {
            self.hits.push(RailHit {
                file: self.file.to_path_buf(),
                kind: "validate_dep_signature-call",
                detail: "`.validate_dep_signature(...)` -- legacy AND-gate validator call".into(),
            });
        }
        syn::visit::visit_expr_method_call(self, c);
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if self.cfg_test_depth == 0 {
            if let syn::Expr::Path(p) = &*c.func {
                if p.path
                    .segments
                    .last()
                    .map(|s| s.ident == "validate_dep_signature")
                    .unwrap_or(false)
                {
                    self.hits.push(RailHit {
                        file: self.file.to_path_buf(),
                        kind: "validate_dep_signature-call",
                        detail: "`validate_dep_signature(...)` -- legacy AND-gate validator call"
                            .into(),
                    });
                }
            }
        }
        syn::visit::visit_expr_call(self, c);
    }

    fn visit_expr_field(&mut self, e: &'ast syn::ExprField) {
        if self.cfg_test_depth == 0 {
            if let syn::Member::Named(name) = &e.member {
                if name == "legacy" {
                    self.hits.push(RailHit {
                        file: self.file.to_path_buf(),
                        kind: "legacy-field-access",
                        detail: "`<expr>.legacy` -- read of the retired cache-validity rail".into(),
                    });
                }
            }
        }
        syn::visit::visit_expr_field(self, e);
    }
}

/// `#[cfg(test)]` detection — token-scan the `cfg(...)` payload for the
/// bare `test` identifier (mirrors the sibling guards).
fn has_cfg_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        let rendered = match &a.meta {
            syn::Meta::List(list) => list.tokens.to_string(),
            _ => return false,
        };
        rendered
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|t| t == "test")
    })
}

fn scan_file_rails(path: &Path, hits: &mut Vec<RailHit>) {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed =
        syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let mut scanner = RailScanner {
        file: path,
        cfg_test_depth: 0,
        hits,
    };
    scanner.visit_file(&parsed);
}

// ---------------------------------------------------------------------------
// Production-tree guards.
// ---------------------------------------------------------------------------

/// Legacy `DepSignature` field on `ReadSetSignature` (and any other
/// cache-carrier struct) is gone.
///
/// Cache validity collapsed to one oracle — the path-precise
/// fact-tracer signature `ReadSetSignature.facts`. No cache-carrier
/// struct (`ReadSetSignature` or any `*Entry` / `*CacheEntry` /
/// `*Signature` / `*Snapshot`) may carry a public `DepSignature`
/// field. Re-introducing one resurrects the retired bundled rail.
#[test]
fn no_legacy_dep_signature_field_in_cache_carriers() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "`legacy_dep_signature_field_gone` violation:\n{}\n\n\
         The bundled `DepSignature` cache-validity rail is retired. \n\
         No cache-carrier struct may carry a public `DepSignature` \n\
         field — the path-precise fact-tracer \n\
         (`facts: Arc<[FactVersionRef]>`) is the sole cache-validity \n\
         authority.",
        format_violations(&violations)
    );
}

/// The legacy cache-validity rail — the `validate_dep_signature`
/// AND-gate validator and every `.legacy` field read — is gone from
/// production source.
///
/// `validate_dep_signature` was the bundled whole-hash /
/// project-generation validator AND-gated alongside fact validation;
/// `.legacy` was the `ReadSetSignature` field carrying that rail. Both
/// are retired: fact validation (`validates_fact_signature` /
/// `validate_with_self_roots` over `ReadSetSignature.facts`) is the
/// sole cache-validity oracle. A re-introduced `validate_dep_signature`
/// definition / call, or a `.legacy` read, is a regression.
#[test]
fn no_legacy_dep_signature_validation_rail_in_production() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut hits = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file_rails(&file, &mut hits);
    }
    let mut by_file: BTreeMap<&Path, Vec<&RailHit>> = BTreeMap::new();
    for h in &hits {
        by_file.entry(h.file.as_path()).or_default().push(h);
    }
    let mut rendered = Vec::new();
    for (file, hs) in by_file {
        rendered.push(format!("  {}", file.display()));
        for h in hs {
            rendered.push(format!("    [{}] {}", h.kind, h.detail));
        }
    }
    assert!(
        hits.is_empty(),
        "`legacy_dep_signature_field_gone` validation-rail violation:\n{}\n\n\
         The legacy `DepSignature` cache-validity rail is retired. \n\
         Production cache code must not define or call \n\
         `validate_dep_signature`, nor read a `.legacy` rail. Fact \n\
         validation over `ReadSetSignature.facts` is the sole \n\
         cache-validity oracle.",
        rendered.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Sentinel: discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Drive the scanner against synthetic fixtures so the
/// production-tree guard cannot pass trivially when un-ignored.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: `pub legacy: DepSignature` on a cache carrier — REJECTED.
    let fixture_a = r#"
        pub struct ReadSetSignature {
            pub facts: Arc<[FactVersionRef]>,
            pub legacy: DepSignature,
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_a).is_empty(),
        "scanner failed to flag `pub legacy: DepSignature` on ReadSetSignature"
    );

    // Fixture B: `pub legacy: DepSignature` on a non-carrier struct — ACCEPTED.
    // (The bare-suffix rule keys on cache-carrier names; a struct named
    // `Foo` is not in scope.)
    let fixture_b = r#"
        pub struct Foo {
            pub legacy: DepSignature,
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_b).is_empty(),
        "scanner incorrectly flagged `DepSignature` on a non-carrier struct: {:?}",
        scan_fixture_violations(fixture_b)
    );

    // Fixture C: private `legacy: DepSignature` on a carrier — ACCEPTED.
    // The guard targets the *public* migration rail. A private field
    // with that type would be a separate (and far smaller) concern.
    let fixture_c = r#"
        pub struct ReadSetSignature {
            legacy: DepSignature,
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_c).is_empty(),
        "scanner incorrectly flagged a non-pub field: {:?}",
        scan_fixture_violations(fixture_c)
    );

    // Fixture D: cache-carrier struct WITHOUT a `DepSignature` field — ACCEPTED.
    let fixture_d = r#"
        pub struct ReadSetSignature {
            pub facts: Arc<[FactVersionRef]>,
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_d).is_empty(),
        "scanner incorrectly flagged a clean ReadSetSignature: {:?}",
        scan_fixture_violations(fixture_d)
    );

    // Fixture E: `*Entry` suffix carrier — REJECTED.
    let fixture_e = r#"
        pub struct MaterializeStructureEntry {
            pub dep_signature: DepSignature,
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_e).is_empty(),
        "scanner failed to flag DepSignature on a *Entry carrier struct"
    );

    // Fixture F: `Vec<DepSignature>` or `Option<DepSignature>` etc. —
    // NOT matched. The guard targets the bare typename only; wrapped
    // forms are a different shape that would require a separate rule.
    let fixture_f = r#"
        pub struct ReadSetSignature {
            pub legacy_set: Vec<DepSignature>,
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_f).is_empty(),
        "scanner unexpectedly flagged a wrapped DepSignature container: {:?}",
        scan_fixture_violations(fixture_f)
    );
}

fn scan_fixture_violations(src: &str) -> Vec<Violation> {
    let parsed = syn::parse_file(src).expect("parse fixture");
    let mut violations = Vec::new();
    let fake_path = Path::new("<fixture>");
    let mut scanner = Scanner {
        file: fake_path,
        violations: &mut violations,
    };
    scanner.visit_file(&parsed);
    violations
}

/// Drive the validation-rail scanner against synthetic fixtures so
/// `no_legacy_dep_signature_validation_rail_in_production` cannot pass
/// trivially.
#[test]
fn rail_scanner_discriminating_property_fixtures() {
    // Fixture A: free-function `validate_dep_signature` definition — REJECTED.
    let a = r#"
        fn validate_dep_signature(sig: &DepSignature) -> bool { sig.is_empty() }
    "#;
    assert!(
        !scan_fixture_rails(a).is_empty(),
        "rail scanner failed to flag a `validate_dep_signature` definition"
    );

    // Fixture B: trait-method `validate_dep_signature` — REJECTED.
    let b = r#"
        trait T {
            fn validate_dep_signature(&self, sig: &DepSignature) -> bool;
        }
    "#;
    assert!(
        !scan_fixture_rails(b).is_empty(),
        "rail scanner failed to flag a `validate_dep_signature` trait method"
    );

    // Fixture C: `.validate_dep_signature(...)` method call — REJECTED.
    let c = r#"
        fn caller(ctx: &dyn R) -> bool { ctx.validate_dep_signature(&sig) }
    "#;
    assert!(
        !scan_fixture_rails(c).is_empty(),
        "rail scanner failed to flag a `.validate_dep_signature(...)` call"
    );

    // Fixture D: `<expr>.legacy` field read — REJECTED.
    let d = r#"
        fn caller(entry: &E) -> usize { entry.read_set_signature.legacy.len() }
    "#;
    assert!(
        !scan_fixture_rails(d).is_empty(),
        "rail scanner failed to flag a `.legacy` field read"
    );

    // Fixture E: clean fact-only code — ACCEPTED.
    let e = r#"
        fn caller(entry: &E, ctx: &dyn R) -> bool {
            entry.read_set_signature.validate_with_self_roots(ctx, &roots)
        }
    "#;
    assert!(
        scan_fixture_rails(e).is_empty(),
        "rail scanner incorrectly flagged clean fact-only code: {:?}",
        scan_fixture_rails(e)
    );

    // Fixture F: `#[cfg(test)] mod` containing both shapes — ACCEPTED
    // (test scaffolding is exempt, mirroring the sibling guards).
    let f = r#"
        #[cfg(test)]
        mod tests {
            fn t(ctx: &dyn R, entry: &E) -> bool {
                let _ = entry.read_set_signature.legacy.len();
                ctx.validate_dep_signature(&sig)
            }
        }
    "#;
    assert!(
        scan_fixture_rails(f).is_empty(),
        "rail scanner incorrectly flagged a #[cfg(test)] mod body: {:?}",
        scan_fixture_rails(f)
    );

    // Fixture G: an unrelated `.legacy_set` field — NOT matched (the
    // guard keys on the exact member name `legacy`).
    let g = r#"
        fn caller(x: &X) -> usize { x.legacy_set.len() }
    "#;
    assert!(
        scan_fixture_rails(g).is_empty(),
        "rail scanner incorrectly flagged an unrelated `.legacy_set` field: {:?}",
        scan_fixture_rails(g)
    );
}

fn scan_fixture_rails(src: &str) -> Vec<RailHit> {
    let parsed = syn::parse_file(src).expect("parse fixture");
    let mut hits = Vec::new();
    let fake_path = Path::new("<fixture>");
    let mut scanner = RailScanner {
        file: fake_path,
        cfg_test_depth: 0,
        hits: &mut hits,
    };
    scanner.visit_file(&parsed);
    hits
}
