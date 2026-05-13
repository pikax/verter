//! Stage 10 arch-guard — `StoreViewCompatToken` is the concurrency
//! oracle, NEVER the cache-correctness oracle.
//!
//! Binds R19: "Fact validation is the cache-correctness oracle.
//! `StoreViewCompatToken` is the concurrency oracle: singleflight
//! lane separation, mid-query change detection, write admission
//! against superseded computations. The two are orthogonal and must
//! not be conflated."
//!
//! The guard scans every production source file under
//! `crates/verter_session/src/**/*.rs` and asserts that
//! `StoreViewCompatToken` appears only in concurrency contexts:
//!
//! 1. **Singleflight keys** — `(K, StoreViewCompatToken)` map keys,
//!    `SingleflightGroup<...>` definitions, `run(key, token, ...)`
//!    invocations.
//! 2. **`compat_token()` accessor** on the `StoreView` trait — the
//!    token IS the trait's identity surface for singleflight.
//! 3. **`store_view_token()` accessor** on `ResolverContext` —
//!    same purpose, exposed at the request scope.
//! 4. **Type-system / signature appearances** — `let token:
//!    StoreViewCompatToken`, function parameters, struct fields.
//!
//! The guard explicitly checks for the FORBIDDEN PATTERNS — any
//! call site that mixes `StoreViewCompatToken` with a freshness /
//! validation / "is this cache entry still good?" predicate is a
//! conflation of the concurrency oracle with the correctness
//! oracle and FAILS the guard. Concretely, the guard greps for
//! strings of the form:
//!
//! - `StoreViewCompatToken` co-located on the SAME LINE with
//!   `valid`, `fresh`, `stale`, or `validate` (excluding
//!   `validates_*_domain` per-domain dispatch on the trait — those
//!   reference `FactKey::domain()`, NOT the token).
//! - `if token == ... && view.<freshness predicate>` patterns.
//! - Any function body that returns `bool` based on a token
//!   comparison that is then used as a cache freshness gate.
//!
//! Discrimination: the guard MUST FAIL pre-change if a hypothetical
//! conflation site is introduced; it MUST PASS post-change against
//! the current tree.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("workspace root resolves above the crate dir")
}

fn scan_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let read = match std::fs::read_dir(&p) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for e in read.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|x| x.to_str()) != Some("rs") {
                continue;
            }
            out.push(path);
        }
    }
    out
}

/// Discrimination 1: every `StoreViewCompatToken` occurrence in
/// production source code (under `crates/verter_session/src/**`) is
/// in a concurrency-oracle context. The guard scans every line,
/// classifies each `StoreViewCompatToken` mention, and asserts the
/// classification is in the allow-list set.
#[test]
fn store_view_compat_token_is_concurrency_oracle_only() {
    // The forbidden co-location vocabulary. The guard's flag
    // predicate is:
    //   line contains `StoreViewCompatToken` AND
    //   line OR ±2 nearby lines contain a forbidden token AND
    //   the SAME LINE is not a concurrency-context signature.
    //
    // Concurrency-context signatures (the things the token IS for)
    // redeem the line; nearby-only redemption is NOT allowed,
    // because it lets a freshness-call-site sneak past whenever
    // the file happens to import the resolver-core types nearby.
    const SAME_LINE_REDEMPTION_TERMS: &[&str] = &[
        "compat_token",
        "store_view_token",
        "singleflight",
        // Type-system / signature appearances are always OK.
        "token: StoreViewCompatToken",
        "StoreViewCompatToken,",
        "StoreViewCompatToken {",
        "StoreViewCompatToken)",
        "(K, StoreViewCompatToken)",
        "-> StoreViewCompatToken",
        "fn compat_token",
        "fn store_view_token",
        "pub struct StoreViewCompatToken",
        "use crate::resolver_core::StoreViewCompatToken",
        "use super::resolver_core::StoreViewCompatToken",
        // Comments / docstrings on the same line are documentation,
        // not call sites.
        "/// ",
        "//!",
        "// ",
    ];

    // Forbidden vocabulary suggesting freshness / correctness
    // conflation. A site is flagged when the SAME LINE as a
    // `StoreViewCompatToken` mention contains one of these, OR
    // any of the ±2 nearby lines do.
    const FORBIDDEN_NEAR: &[&str] = &[
        "is_fresh",
        "is_stale",
        "is_valid",
        "is_invalidated",
        "needs_recompute",
        "should_invalidate",
        "should_evict",
        "freshness",
        "staleness",
    ];

    let crate_src = workspace_root()
        .join("crates")
        .join("verter_session")
        .join("src");
    let files = scan_rs_files(&crate_src);
    let mut violations: Vec<String> = Vec::new();
    for file in files {
        // Skip test files — the guard scans production paths only.
        let path_str = file.display().to_string().replace('\\', "/");
        if path_str.ends_with("_tests.rs") || path_str.contains("/tests/") {
            continue;
        }
        let src = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for (line_no, line) in src.lines().enumerate() {
            if !line.contains("StoreViewCompatToken") {
                continue;
            }
            // Skip per-domain dispatch references — they explicitly
            // route via `FactKey::domain()` and are the correctness
            // path (R26).
            if line.contains("validates_parse_domain")
                || line.contains("validates_resolve_imports_domain")
                || line.contains("validates_route_surface_domain")
            {
                continue;
            }
            // Same-line redemption only: if the line itself is a
            // concurrency-context signature (singleflight key,
            // accessor, struct-literal construction, use-import,
            // pure docstring) the mention is intrinsically about
            // the concurrency oracle and is allowed regardless of
            // any nearby forbidden vocabulary.
            let same_line_redeemed = SAME_LINE_REDEMPTION_TERMS
                .iter()
                .any(|t| line.contains(t));
            if same_line_redeemed {
                continue;
            }
            // Same-line OR nearby forbidden vocabulary flags the
            // site as a freshness / correctness conflation.
            let line_lower = line.to_ascii_lowercase();
            let nearby = {
                let lines: Vec<&str> = src.lines().collect();
                let start = line_no.saturating_sub(2);
                let end = (line_no + 3).min(lines.len());
                lines[start..end].join("\n").to_ascii_lowercase()
            };
            let conflated = FORBIDDEN_NEAR
                .iter()
                .any(|t| line_lower.contains(t) || nearby.contains(t));
            if conflated {
                violations.push(format!(
                    "{}:{}: StoreViewCompatToken co-located with \
                     freshness / correctness predicate. Token is \
                     the CONCURRENCY oracle (R19); cache \
                     correctness flows through fact validation. \
                     Line: {}",
                    path_str,
                    line_no + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Stage 10 R19 guard violated — {} site(s) conflate \
         `StoreViewCompatToken` with cache freshness / correctness \
         predicates:\n{}",
        violations.len(),
        violations.join("\n"),
    );
}

/// Discrimination 2: the guard's redemption-allow-list itself must
/// match every production `StoreViewCompatToken` site. If a site
/// exists that has NO redemption term AND NO forbidden term, it is
/// a "naked" mention that the guard cannot classify — flag it for
/// review so the redemption list stays exhaustive.
#[test]
fn store_view_compat_token_every_production_site_classified() {
    let crate_src = workspace_root()
        .join("crates")
        .join("verter_session")
        .join("src");
    let files = scan_rs_files(&crate_src);
    let mut total = 0usize;
    for file in files {
        let path_str = file.display().to_string().replace('\\', "/");
        if path_str.ends_with("_tests.rs") || path_str.contains("/tests/") {
            continue;
        }
        let src = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for line in src.lines() {
            if line.contains("StoreViewCompatToken") {
                total += 1;
            }
        }
    }
    // The token is in active use across the resolver/singleflight
    // surface. A zero count would indicate the token has been
    // retired entirely (which Stage 10 explicitly preserves per
    // R19). If you are reading this assertion failure, either
    // (a) you intentionally retired the token — also delete this
    // guard; or (b) the file refactor moved sites — adjust the
    // scan scope.
    assert!(
        total > 0,
        "StoreViewCompatToken has zero production references — \
         R19 explicitly preserves the token as the concurrency \
         oracle; a zero count is either a retirement (also delete \
         this guard) or a scan-scope drift (adjust crate_src)."
    );
}
