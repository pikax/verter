//! Architecture guard — `VerterHost` public invalidation surface.
//!
//! `VerterHost`'s invalidation surface must stay narrow. The fact-based
//! cache architecture revalidates warm hits via `HostFenceValidator`
//! and dep-signatures, so external callers should NOT be able to
//! issue arbitrary cache resets through `VerterHost`. The documented
//! narrow public API on `impl VerterHost` is:
//!
//! - `invalidate_compile_slots(canonical_or_alias)` — targeted
//!   per-file invalidation of compile slots (canonical-keyed).
//!
//! Internal lifecycle reset producers (`configure_projects`,
//! `upsert_via_scheduler_with_options`, `clear_compile_cache`, etc.)
//! are explicitly allow-listed below — those are the documented
//! lifecycle / project-graph reset entry points and they have always
//! lived on the public `VerterHost` surface.
//!
//! Any OTHER `pub fn invalidate*` / `pub fn purge*` / `pub fn wipe*` /
//! `pub fn drop_all*` method on `impl VerterHost` is a regression —
//! either pull the method down to `pub(crate)` (consumer-internal) or
//! re-route the consumer through the documented narrow API.
//!
//! Note: this guard is `VerterHost`-scoped. The many `pub fn
//! invalidate_*` methods on cache types (`SemanticGraphStore`,
//! `MaterializeStructureDb`, `MemberSemanticFactStore`, etc.) are
//! NOT in scope — those caches are internal substrate accessed
//! through `ProjectTypeStore` and do not expose a top-level public
//! surface to external callers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{ImplItemFn, ItemImpl, Visibility};
use walkdir::WalkDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// `pub fn` method names allowed on `impl VerterHost` whose name
/// starts with an invalidation-shaped prefix. Anything else flagged.
const ALLOWED_VERTER_HOST_INVALIDATION_FNS: &[&str] = &[
    // Documented narrow public API — targeted, per-canonical
    // invalidation of compile slots only.
    "invalidate_compile_slots",
];

/// Method-name prefixes the guard treats as "invalidation-shaped". A
/// `pub fn` on `impl VerterHost` matching one of these prefixes (other
/// than `ALLOWED_VERTER_HOST_INVALIDATION_FNS`) is flagged.
///
/// Note we deliberately do NOT flag bare names like `close`, `evict`,
/// or `clear_*`. Those are lifecycle reset producers (already covered
/// by the import_route_writer_guard.rs `RESET_NAME_PREFIXES` rule), not
/// invalidation surface in the sense this guard targets.
const FLAG_PREFIXES: &[&str] = &["invalidate", "purge", "wipe", "drop_all"];

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    method: String,
    detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: `pub fn {}` on `impl VerterHost` -- {}",
            self.file.display(),
            self.method,
            self.detail
        )
    }
}

struct Scanner<'a> {
    file: &'a Path,
    /// `true` once the visitor enters an `impl VerterHost { ... }`
    /// (free inherent impl, NOT a trait impl).
    inside_verter_host_inherent_impl: bool,
    violations: &'a mut Vec<Violation>,
}

impl<'a> Scanner<'a> {
    fn new(file: &'a Path, violations: &'a mut Vec<Violation>) -> Self {
        Self {
            file,
            inside_verter_host_inherent_impl: false,
            violations,
        }
    }
}

fn is_verter_host_inherent_impl(i: &ItemImpl) -> bool {
    // Only inherent impls — trait impls have `i.trait_ = Some(...)` and
    // their public method-name surface is constrained by the trait, so
    // those don't expand the host's invalidation API in a way the guard
    // is meant to catch.
    if i.trait_.is_some() {
        return false;
    }
    match &*i.self_ty {
        syn::Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map(|s| s.ident == "VerterHost")
            .unwrap_or(false),
        _ => false,
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        let prev = self.inside_verter_host_inherent_impl;
        if is_verter_host_inherent_impl(i) {
            self.inside_verter_host_inherent_impl = true;
        }
        syn::visit::visit_item_impl(self, i);
        self.inside_verter_host_inherent_impl = prev;
    }

    fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
        if !self.inside_verter_host_inherent_impl {
            return;
        }
        // Only `pub fn ...` is in scope; `pub(crate) fn ...` /
        // `pub(super) fn ...` / private `fn ...` do not widen the
        // external surface.
        if !matches!(f.vis, Visibility::Public(_)) {
            return;
        }
        let name = f.sig.ident.to_string();
        if ALLOWED_VERTER_HOST_INVALIDATION_FNS.contains(&name.as_str()) {
            return;
        }
        let triggered = FLAG_PREFIXES.iter().any(|p| name.starts_with(p));
        if !triggered {
            return;
        }
        self.violations.push(Violation {
            file: self.file.to_path_buf(),
            method: name,
            detail: format!(
                "added new public invalidation surface (prefix matched: \
                 one of `{}`); allowed: {}. Either rename / pull down to \
                 `pub(crate)`, or extend `ALLOWED_VERTER_HOST_INVALIDATION_FNS` \
                 with a justification.",
                FLAG_PREFIXES.join("`, `"),
                ALLOWED_VERTER_HOST_INVALIDATION_FNS.join(", "),
            ),
        });
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
            lines.push(format!("    pub fn `{}`: {}", v.method, v.detail));
        }
    }
    format!(
        "found {} VerterHost public invalidation-surface violation(s):\n{}",
        violations.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Production-tree guard.
// ---------------------------------------------------------------------------

/// `impl VerterHost` may expose only `invalidate_compile_slots` as a
/// `pub fn` whose name starts with `invalidate*` / `purge*` / `wipe*`
/// / `drop_all*`. Any other public invalidation-shaped method widens
/// the external surface in a way that bypasses the fact-based
/// revalidation contract. Fix: pull the method down to `pub(crate)` —
/// or, if it really must be public, document why and extend the
/// allow-list.
#[test]
fn verter_host_public_invalidation_surface_is_narrow() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "Block 5 `invalidation_public_surface_gone` violation:\n{}\n\n\
         `impl VerterHost` exposes a narrow public invalidation API. \n\
         The fact-based cache architecture revalidates warm hits via \n\
         `HostFenceValidator` and dep-signatures, so external callers \n\
         should not be able to invalidate arbitrary caches through \n\
         `VerterHost`. The documented narrow surface is: \n\
         `{}`. If you need new invalidation behaviour, either pull the \n\
         method down to `pub(crate)` or extend the allow-list with a \n\
         justification comment.",
        ALLOWED_VERTER_HOST_INVALIDATION_FNS.join(", "),
        format_violations(&violations)
    );
}

// ---------------------------------------------------------------------------
// Sentinel: pin the existing allow-listed method actually exists.
// ---------------------------------------------------------------------------

/// `invalidate_compile_slots` is the single documented public
/// invalidation method on `impl VerterHost`. If a future refactor
/// deletes it, this sentinel fails so reviewers know the public
/// invalidation contract has shifted.
#[test]
fn invalidate_compile_slots_still_exists_on_verter_host() {
    let path = workspace_root().join("crates/verter_session/src/host_manage/analysis_io.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed = syn::parse_file(&src).expect("parse analysis_io.rs");

    let mut found = false;
    for item in &parsed.items {
        if let syn::Item::Impl(item_impl) = item {
            if !is_verter_host_inherent_impl(item_impl) {
                continue;
            }
            for impl_item in &item_impl.items {
                if let syn::ImplItem::Fn(f) = impl_item {
                    if f.sig.ident == "invalidate_compile_slots"
                        && matches!(f.vis, Visibility::Public(_))
                    {
                        found = true;
                    }
                }
            }
        }
    }
    assert!(
        found,
        "`pub fn invalidate_compile_slots(...)` must exist on \
         `impl VerterHost` in `host_manage/analysis_io.rs`; this is the \
         documented narrow public invalidation API. If you deleted it, \
         update the allow-list in `invalidation_public_surface_gone.rs`."
    );
}

// ---------------------------------------------------------------------------
// Sentinel: discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Drive the scanner against synthetic fixtures to confirm the
/// classification works. Without this, the production-tree guard could
/// pass trivially if the scanner never matched ANY method.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: `pub fn invalidate_everything` on `impl VerterHost` —
    // REJECTED (not in allow-list, prefix matches `invalidate`).
    let fixture_a = r#"
        impl VerterHost {
            pub fn invalidate_everything(&self) {}
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_a).is_empty(),
        "scanner failed to flag `pub fn invalidate_everything` on VerterHost"
    );

    // Fixture B: allow-listed `pub fn invalidate_compile_slots` — ACCEPTED.
    let fixture_b = r#"
        impl VerterHost {
            pub fn invalidate_compile_slots(&self, canonical: &str) {
                let _ = canonical;
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_b).is_empty(),
        "scanner incorrectly flagged allow-listed `invalidate_compile_slots`: {:?}",
        scan_fixture_violations(fixture_b)
    );

    // Fixture C: `pub(crate) fn invalidate_*` is ACCEPTED (not `pub`).
    let fixture_c = r#"
        impl VerterHost {
            pub(crate) fn invalidate_crate_internal(&self) {}
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_c).is_empty(),
        "scanner incorrectly flagged `pub(crate) fn invalidate_*`: {:?}",
        scan_fixture_violations(fixture_c)
    );

    // Fixture D: `pub fn purge_arbitrary` on VerterHost — REJECTED.
    let fixture_d = r#"
        impl VerterHost {
            pub fn purge_arbitrary(&self) {}
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_d).is_empty(),
        "scanner failed to flag `pub fn purge_*` on VerterHost"
    );

    // Fixture E: `pub fn wipe_all` on VerterHost — REJECTED.
    let fixture_e = r#"
        impl VerterHost {
            pub fn wipe_all(&self) {}
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_e).is_empty(),
        "scanner failed to flag `pub fn wipe_all` on VerterHost"
    );

    // Fixture F: `pub fn drop_all_caches` on VerterHost — REJECTED.
    let fixture_f = r#"
        impl VerterHost {
            pub fn drop_all_caches(&self) {}
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_f).is_empty(),
        "scanner failed to flag `pub fn drop_all_caches` on VerterHost"
    );

    // Fixture G: invalidation method on an UNRELATED type — ACCEPTED.
    // The guard is `VerterHost`-scoped; per-cache invalidation methods
    // (e.g. on `SemanticGraphStore`) are NOT in scope.
    let fixture_g = r#"
        impl SemanticGraphStore {
            pub fn invalidate_canonical(&self, canonical: &str) {
                let _ = canonical;
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_g).is_empty(),
        "scanner incorrectly flagged invalidation on a non-VerterHost type: {:?}",
        scan_fixture_violations(fixture_g)
    );

    // Fixture H: trait impl on VerterHost — ACCEPTED (trait impls
    // constrain the surface to the trait definition; not in scope).
    let fixture_h = r#"
        impl SomeTrait for VerterHost {
            pub fn invalidate_via_trait(&self) {}
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_h).is_empty(),
        "scanner incorrectly flagged a trait impl method: {:?}",
        scan_fixture_violations(fixture_h)
    );

    // Fixture I: `pub fn clear_*` on VerterHost — ACCEPTED. `clear_*`
    // is a documented lifecycle reset prefix, not invalidation surface
    // in the sense this guard targets. (The `no_eager_invalidation`
    // sibling guard pins the bulk-clear semantics inside the body.)
    let fixture_i = r#"
        impl VerterHost {
            pub fn clear_compile_cache(&self) {}
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_i).is_empty(),
        "scanner incorrectly flagged a `pub fn clear_*` method: {:?}",
        scan_fixture_violations(fixture_i)
    );

    // Fixture J: `pub fn invalidate_canonical` on VerterHost — REJECTED.
    // Even though `invalidate_canonical` is the documented narrow API on
    // cache types, it is NOT in the VerterHost allow-list — VerterHost's
    // narrow API is `invalidate_compile_slots` specifically.
    let fixture_j = r#"
        impl VerterHost {
            pub fn invalidate_canonical(&self, canonical: &str) {
                let _ = canonical;
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_j).is_empty(),
        "scanner failed to flag `pub fn invalidate_canonical` on VerterHost"
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
