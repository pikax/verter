//! Architecture guard — legacy `DepSignature` field on
//! `ReadSetSignature` (and any other cache-carrier struct) is gone.
//!
//! The Block 6.B / 7 / 8 retirement removes the legacy
//! `ReadSetSignature.legacy: DepSignature` field. The path-precise
//! fact-tracer (`facts: Arc<[FactVersionRef]>`) is the sole
//! cache-validity authority after the retirement; the bundled
//! `DepSignature` cache-validity rail is dead.
//!
//! `#[ignore]` reason: the field still exists at HEAD (commit
//! `e79dbdb54`) and is actively consumed during the dual-emit
//! migration window. Un-ignoring this test before Block 6.B+7+8
//! land would fail — so the guard sits dormant until the retirement
//! commit deletes the field, at which point the `#[ignore]` line
//! is removed alongside the producer.
//!
//! When activated, this guard scans
//! `crates/verter_session/src/**/*.rs` (production source) for any
//! `pub <name>: DepSignature` field declaration inside a struct
//! whose role is "cache carrier" — concretely, any struct named
//! `ReadSetSignature` (the carrier type) or any per-cache entry
//! struct with the suffix `*Entry` / `*CacheEntry` / `*Signature` /
//! `*Snapshot`. Re-introduction of the legacy field is a regression.

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
// Production-tree guard (block-gated).
// ---------------------------------------------------------------------------

/// Legacy `DepSignature` field on `ReadSetSignature` (and any other
/// cache-carrier struct) must be deleted.
///
/// Today the field still exists at HEAD; the producer wires
/// `state.fact_versions` into the legacy rail during the dual-emit
/// migration window. Block 6.B + 7 + 8 retire the legacy rail; this
/// test stays `#[ignore]`'d until that lands. Once the field is
/// deleted, the `#[ignore]` line above this test is removed and the
/// guard activates.
#[test]
#[ignore = "block-6.B+7+8 RED — closed by ReadSetSignature.legacy field deletion"]
fn no_legacy_dep_signature_field_in_cache_carriers() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "Block 5 `legacy_dep_signature_field_gone` violation (gated):\n{}\n\n\
         The legacy `DepSignature` field on cache-carrier structs is \n\
         being retired in Block 6.B / 7 / 8. Once the dual-emit window \n\
         closes, the `ReadSetSignature.legacy: DepSignature` field \n\
         (and any equivalent) must be deleted. The path-precise \n\
         fact-tracer (`facts: Arc<[FactVersionRef]>`) is the sole \n\
         cache-validity authority.",
        format_violations(&violations)
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
