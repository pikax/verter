//! Architecture guard — legacy `DepSignature` field on
//! `ReadSetSignature` (and any other cache-carrier struct) is gone.
//!
//! Cache validity is a single oracle: the path-precise fact-tracer
//! signature `ReadSetSignature.facts: Arc<[FactVersionRef]>`. The
//! bundled whole-hash / project-generation `DepSignature`
//! cache-validity rail — once carried as `ReadSetSignature.legacy` —
//! is retired. No cache-carrier struct may carry a `DepSignature`
//! field under any visibility class except for the single sanctioned
//! sibling `dispatch_dep_signature` field on `MemoEntry` /
//! `MaterializeStructureEntry` / `RefCycleEntry`.
//!
//! This guard scans `crates/verter_session/src/**/*.rs` (production
//! source) for any `<vis> <name>: DepSignature` field declaration
//! inside a struct whose role is "cache carrier" — concretely, any
//! struct named `ReadSetSignature` (the carrier type) or any
//! per-cache entry struct with the suffix `*Entry` / `*CacheEntry` /
//! `*Signature` / `*Snapshot`. The visibility classes the guard
//! treats as cache-rail-shaped are:
//!
//! - `pub` — public cache-validity rail. Always rejected.
//! - `pub(crate)` and `pub(super)` — restricted-visibility
//!   cache-carrier fields. Rejected UNLESS the field name is exactly
//!   `dispatch_dep_signature` (the dispatch-return accumulator
//!   sibling field — not a cache-validity rail). Without this
//!   restricted-visibility coverage a future contributor could
//!   re-introduce a re-named legacy-rail field under `pub(super)` /
//!   `pub(crate)` and slip past the guard.
//! - module-private (no visibility marker) — out of scope. A truly
//!   private field is not part of the cache rail surface a sibling
//!   crate or module could read through.

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

/// Visibility classes the guard considers cache-rail-shaped.
#[derive(Debug, Clone, Copy)]
enum CacheRailVisibility {
    /// `pub` — public cache-validity rail.
    Public,
    /// `pub(crate)` — crate-visible cache-carrier field.
    PubCrate,
    /// `pub(super)` — parent-module-visible cache-carrier field.
    PubSuper,
}

impl CacheRailVisibility {
    fn as_label(self) -> &'static str {
        match self {
            CacheRailVisibility::Public => "pub",
            CacheRailVisibility::PubCrate => "pub(crate)",
            CacheRailVisibility::PubSuper => "pub(super)",
        }
    }
}

/// Classify a `syn::Visibility` as one of the cache-rail-shaped
/// visibility classes the guard targets. Module-private (no marker)
/// and other restricted shapes (`pub(in path)`, `pub(self)`) are out
/// of scope.
fn classify_cache_rail_visibility(vis: &Visibility) -> Option<CacheRailVisibility> {
    match vis {
        Visibility::Public(_) => Some(CacheRailVisibility::Public),
        Visibility::Restricted(r) => {
            if r.path.is_ident("crate") {
                Some(CacheRailVisibility::PubCrate)
            } else if r.path.is_ident("super") {
                Some(CacheRailVisibility::PubSuper)
            } else {
                None
            }
        }
        Visibility::Inherited => None,
    }
}

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    struct_name: String,
    field_name: String,
    visibility: CacheRailVisibility,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: `{} {}.{}: DepSignature` -- legacy-rail-shaped field must be deleted or renamed to `dispatch_dep_signature`",
            self.file.display(),
            self.visibility.as_label(),
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

/// The single sanctioned cache-carrier `DepSignature` field name. The
/// dispatch-return accumulator on `MemoEntry` /
/// `MaterializeStructureEntry` / `RefCycleEntry` is an internal
/// sibling rail (not a cache-validity rail), so it is allowed to live
/// on a cache-carrier struct under `pub(crate)` / `pub(super)`. Any
/// other name under restricted visibility is a regression.
const SANCTIONED_DISPATCH_FIELD: &str = "dispatch_dep_signature";

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
                let Some(visibility) = classify_cache_rail_visibility(&field.vis) else {
                    continue;
                };
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
                // `pub` is always rejected — no sanctioned public
                // cache-carrier `DepSignature` field exists.
                // `pub(crate)` and `pub(super)` are rejected UNLESS
                // the field name is exactly `dispatch_dep_signature`
                // (the dispatch-return accumulator sibling field).
                let allowed = matches!(
                    visibility,
                    CacheRailVisibility::PubCrate | CacheRailVisibility::PubSuper
                ) && field_name == SANCTIONED_DISPATCH_FIELD;
                if allowed {
                    continue;
                }
                self.violations.push(Violation {
                    file: self.file.to_path_buf(),
                    struct_name: struct_name.clone(),
                    field_name: field_name.to_string(),
                    visibility,
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
    // Textual pre-filter (coverage-identical): the scanner flags only fields
    // whose type's last path segment is `DepSignature`, so the file must
    // contain that identifier substring to host a violation. The hard
    // parse-error panic is preserved for files that pass the filter.
    if !src.contains("DepSignature") {
        return;
    }
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
                "    struct `{}` field `{} {}: DepSignature`",
                v.struct_name,
                v.visibility.as_label(),
                v.field_name,
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
    // Textual pre-filter (coverage-identical): the rail scanner flags only
    // `validate_dep_signature` call/path sites and `<expr>.legacy` field
    // accesses; either requires its identifier substring to be present, so a
    // file with neither cannot host a hit. Parse-error panic preserved for
    // files that pass the filter.
    if !src.contains("validate_dep_signature") && !src.contains("legacy") {
        return;
    }
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
/// `*Signature` / `*Snapshot`) may carry a `DepSignature` field under
/// `pub`, `pub(crate)`, or `pub(super)` visibility, except for the
/// single sanctioned `dispatch_dep_signature` sibling field on
/// `MemoEntry` / `MaterializeStructureEntry` / `RefCycleEntry`
/// (allowed under restricted visibility only — never `pub`).
/// Re-introducing one resurrects the retired bundled rail.
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
         No cache-carrier struct may carry a `DepSignature` field \n\
         under `pub`, `pub(crate)`, or `pub(super)` visibility \n\
         (except the sanctioned `dispatch_dep_signature` sibling \n\
         under restricted visibility). The path-precise fact-tracer \n\
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

    // Fixture C: module-private `legacy: DepSignature` on a carrier
    // — ACCEPTED. The guard targets the cache-rail-shaped visibility
    // classes (`pub`, `pub(crate)`, `pub(super)`); a truly private
    // field is not reachable through the cache rail surface.
    let fixture_c = r#"
        pub struct ReadSetSignature {
            legacy: DepSignature,
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_c).is_empty(),
        "scanner incorrectly flagged a module-private field: {:?}",
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

    // Fixture G: `pub(super) <renamed>: DepSignature` on a cache
    // carrier — REJECTED. A re-named legacy-rail field under
    // restricted visibility must NOT slip past the guard. This is the
    // "guard escape hatch" the strengthened scanner closes: a
    // contributor introducing a `pub(super) cache_validity_rail:
    // DepSignature` (or any name other than `dispatch_dep_signature`)
    // gets caught.
    let fixture_g = r#"
        pub struct MemoEntry {
            pub(super) cache_validity_rail: DepSignature,
        }
    "#;
    let violations_g = scan_fixture_violations(fixture_g);
    assert!(
        !violations_g.is_empty(),
        "scanner failed to flag `pub(super)` cache-carrier `DepSignature` \
         field whose name is NOT `dispatch_dep_signature`. The new \
         restricted-visibility rail must be caught: {:?}",
        violations_g
    );
    assert!(
        violations_g
            .iter()
            .any(|v| v.field_name == "cache_validity_rail"),
        "scanner flagged something, but not the offending field: {:?}",
        violations_g
    );

    // Fixture H: `pub(crate) <renamed>: DepSignature` on a cache
    // carrier — REJECTED. Same coverage as fixture G for the
    // `pub(crate)` visibility class.
    let fixture_h = r#"
        pub struct MaterializeStructureEntry {
            pub(crate) bundled_dep_rail: DepSignature,
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_h).is_empty(),
        "scanner failed to flag `pub(crate)` cache-carrier `DepSignature` \
         field whose name is NOT `dispatch_dep_signature`"
    );

    // Fixture I: `pub(super) dispatch_dep_signature: DepSignature` on
    // a cache carrier — ACCEPTED. The single sanctioned restricted-
    // visibility cache-carrier `DepSignature` field. This is the
    // exact production shape on `MemoEntry`.
    let fixture_i = r#"
        pub(super) struct MemoEntry {
            pub(super) dispatch_dep_signature: DepSignature,
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_i).is_empty(),
        "scanner incorrectly flagged the sanctioned \
         `pub(super) dispatch_dep_signature: DepSignature` sibling field: {:?}",
        scan_fixture_violations(fixture_i)
    );

    // Fixture J: `pub(crate) dispatch_dep_signature: DepSignature`
    // on a cache carrier — ACCEPTED. The same sanctioned name under
    // the `pub(crate)` visibility class. This is the exact production
    // shape on `MaterializeStructureEntry` / `RefCycleEntry`.
    let fixture_j = r#"
        pub struct MaterializeStructureEntry {
            pub(crate) dispatch_dep_signature: DepSignature,
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_j).is_empty(),
        "scanner incorrectly flagged the sanctioned \
         `pub(crate) dispatch_dep_signature: DepSignature` sibling field: {:?}",
        scan_fixture_violations(fixture_j)
    );

    // Fixture K: `pub dispatch_dep_signature: DepSignature` — REJECTED.
    // The sanctioned name is allowed ONLY under restricted visibility;
    // promoting it to `pub` would expose the dispatch-return rail as
    // a public cache-validity field, which is exactly the public rail
    // the retirement deletes.
    let fixture_k = r#"
        pub struct MemoEntry {
            pub dispatch_dep_signature: DepSignature,
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_k).is_empty(),
        "scanner failed to flag `pub dispatch_dep_signature: DepSignature` \
         — the sanctioned name is restricted-visibility-only, never `pub`"
    );

    // Fixture L: `pub(in some::path) <name>: DepSignature` — NOT
    // matched. The guard only classifies `pub(crate)` and `pub(super)`
    // as cache-rail-shaped restricted visibility. A `pub(in ...)`
    // field is a separate scope shape that the guard does not target
    // (and that production code does not currently use).
    let fixture_l = r#"
        pub struct ReadSetSignature {
            pub(in crate::resolver_core) leaked_rail: DepSignature,
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_l).is_empty(),
        "scanner unexpectedly flagged `pub(in path)` (out of scope): {:?}",
        scan_fixture_violations(fixture_l)
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
