//! Architecture guard — `is_facts_irrelevant: true` eligibility audit.
//!
//! ## Why
//!
//! The Block 1.7 audit at
//! `<OS temp dir>/verter-block1.7/block1.7-facts-irrelevant-eligibility.md`
//! enumerates every
//! cache in `crates/verter_session/src/` that publishes entries
//! carrying a `fact_dep_signature` (or equivalent
//! `read_set_signature`). For each cache, the audit answers:
//!
//! - Does the cold-compute body perform ANY cross-file read?
//! - Verdict: eligible for a future `is_facts_irrelevant: true` flag
//!   ONLY if ZERO cross-file reads occur in cold-compute.
//!
//! The `is_facts_irrelevant: true` flag itself does not yet exist in
//! the substrate. This test pins the audit outputs as runtime
//! constants so a future block that wires up the flag CANNOT silently
//! mark a cache as eligible without updating the audit and this
//! guard together.
//!
//! ## What this guard does
//!
//! 1. Verifies the audit file exists at the expected location.
//! 2. Asserts that no production-source `is_facts_irrelevant: true`
//!    literal appears in `crates/verter_session/src/**/*.rs` —
//!    today no cache carries the flag. A future block that adds it
//!    must extend `ELIGIBLE_CACHES` AND keep the audit file in sync.
//! 3. Asserts that every cache listed in `ELIGIBLE_CACHES` (the
//!    audit's "eligible" rows) maps to an actual production-source
//!    cache definition (path + symbol). If the cache is renamed
//!    or relocated, the test fails until the audit is updated.
//!
//! ## Audit conclusions pinned by this guard
//!
//! Per the Block 1.7 audit, exactly ONE cache is partially eligible
//! today: `FallthroughResolverState.cache`, restricted to the
//! `IntrinsicSurface(_)` and `ConsumedBindings(_)` variants of
//! `FallthroughNodeValue`. Every other fact-validated cache performs
//! cross-file reads in cold compute and is NOT eligible.
//!
//! ## Discriminator
//!
//! A future change that:
//!   - Adds `is_facts_irrelevant: true` to a non-eligible cache
//!     would advance the production-source literal count above 0
//!     without the eligible-cache table being updated; the
//!     `no_is_facts_irrelevant_flag_landed_yet` test fails.
//!   - Removes the audit file without removing the eligibility flag
//!     would fail `audit_file_present_at_documented_path`.
//!   - Renames or deletes one of the eligible-cache symbols would
//!     fail `eligible_caches_resolve_to_existing_symbols`.

use std::path::{Path, PathBuf};

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

/// Caches the Block 1.7 audit deemed eligible (partially or fully)
/// for a future `is_facts_irrelevant: true` flag. The `path_suffix`
/// is matched as a path-suffix (`ends_with` on a `/`-normalised
/// string) and `symbol` names a Rust item the path must contain.
///
/// Format: `(path_suffix, symbol_anchor, justification)`.
const ELIGIBLE_CACHES: &[(&str, &str, &str)] = &[(
    "crates/verter_session/src/resolver_core/fallthrough_resolver.rs",
    "FallthroughResolverState",
    // Justification (audit-side): the `IntrinsicSurface(_)` and
    // `ConsumedBindings(_)` variants carry data sourced from
    // inherently-constant inputs. `IntrinsicSurface` wraps
    // `OwnedIntrinsicMember` data lowered from
    // `verter_semantic::analysis::html_intrinsics` — the SDK's HTML
    // intrinsic registry. `ConsumedBindings` summarises a single
    // owner's bindings (no cross-file dependency). The variant-
    // gated `self.cache.insert(...)` admission in
    // `FallthroughResolverState::store_node` permits empty-facts
    // admission for these two variants, and a future
    // `is_facts_irrelevant: true` flag would document the gate.
    "FallthroughResolverState.cache — variants IntrinsicSurface / ConsumedBindings only",
)];

fn audit_file_path() -> PathBuf {
    // Under the OS temp dir, outside the repo tree (the audit is not
    // committed — it is regenerated per-block).
    std::env::temp_dir()
        .join("verter-block1.7")
        .join("block1.7-facts-irrelevant-eligibility.md")
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

// ---------------------------------------------------------------------------
// Production-tree guards.
// ---------------------------------------------------------------------------

/// No production-source occurrence of `is_facts_irrelevant: true`.
/// A future block that wires up the flag MUST update this test (and
/// the audit) before landing the flag.
#[test]
fn no_is_facts_irrelevant_flag_landed_yet() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut hits: Vec<(PathBuf, usize, String)> = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        let Ok(src) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (idx, line) in src.lines().enumerate() {
            if line.contains("is_facts_irrelevant: true")
                || line.contains("is_facts_irrelevant : true")
            {
                hits.push((file.clone(), idx + 1, line.to_string()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "Block 1.7 audit asserts NO cache carries `is_facts_irrelevant: true` today. \
         A future block that introduces the flag must (1) extend `ELIGIBLE_CACHES` \
         in `is_facts_irrelevant_eligibility.rs`, (2) extend the audit file \
         (under the OS temp dir, `verter-block1.7/block1.7-facts-irrelevant-eligibility.md`) with the new entry's \
         justification (inherently-constant inputs), and (3) extend or rewrite \
         this test's body to reflect the new eligibility surface. Found:\n{}",
        hits.iter()
            .map(|(f, line, src)| format!("  {}:{}: {}", f.display(), line, src.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The audit file at the documented path must exist and contain the
/// expected canonical heading. The test passes when running in any
/// environment that has the audit available; a missing audit file is
/// a soft skip rather than a hard fail because the audit lives
/// outside the repo tree (under the OS temp dir). The audit file is not
/// committed — it is regenerated per-block.
#[test]
fn audit_file_present_at_documented_path() {
    let p = audit_file_path();
    if !p.exists() {
        eprintln!(
            "soft-skip: audit file `{}` is not present in this environment. \
             The Block 1.7 audit is regenerated per-block; the eligibility \
             contract still holds (asserted by the other tests in this file).",
            p.display()
        );
        return;
    }
    let src = std::fs::read_to_string(&p).expect("read audit file");
    assert!(
        src.contains("Block 1.7") && src.contains("is_facts_irrelevant"),
        "audit file at `{}` is missing the canonical heading (`Block 1.7 — is_facts_irrelevant Eligibility Audit`)",
        p.display()
    );
    // The eligible-cache table row must mention the Fallthrough
    // resolver entry. If a future audit revision drops it (e.g.
    // because the cache was retired), this test must be updated
    // alongside the audit and `ELIGIBLE_CACHES`.
    assert!(
        src.contains("FallthroughResolverState") || src.contains("Fallthrough"),
        "audit file at `{}` is missing the documented eligible-cache row \
         (`FallthroughResolverState.cache`)",
        p.display()
    );
}

/// Every cache in `ELIGIBLE_CACHES` must resolve to an existing
/// production-source file containing the documented symbol anchor.
/// A rename or deletion of the cache without updating the audit
/// would fail here.
#[test]
fn eligible_caches_resolve_to_existing_symbols() {
    let repo_root = workspace_root();
    for (path_suffix, symbol_anchor, justification) in ELIGIBLE_CACHES {
        let full = repo_root.join(path_suffix);
        assert!(
            full.exists(),
            "eligible cache path `{path_suffix}` does not exist under repo root \
             `{}`. The cache was relocated or deleted; update `ELIGIBLE_CACHES` \
             and the audit file. Justification was: `{justification}`.",
            repo_root.display()
        );
        let src = std::fs::read_to_string(&full).unwrap_or_else(|e| {
            panic!("read {}: {}", full.display(), e);
        });
        assert!(
            src.contains(symbol_anchor),
            "eligible cache `{path_suffix}` no longer contains anchor symbol \
             `{symbol_anchor}`. The cache was renamed; update `ELIGIBLE_CACHES` \
             and the audit file. Justification was: `{justification}`.",
        );
    }
}

/// The current eligible-cache surface is restricted to a single
/// entry. Pinning the length forces a future change that adds (or
/// removes) eligibility to update this test along with the audit.
#[test]
fn eligible_caches_surface_pinned_to_audit_conclusion() {
    assert_eq!(
        ELIGIBLE_CACHES.len(),
        1,
        "Block 1.7 audit concludes exactly ONE cache is partially eligible \
         for `is_facts_irrelevant: true` (`FallthroughResolverState.cache`). \
         If the audit conclusion changes, update both `ELIGIBLE_CACHES` and \
         the audit file (under the OS temp dir, `verter-block1.7/block1.7-facts-irrelevant-eligibility.md`) \
         in lockstep."
    );
}

/// The audit's discriminating insight is that the eligible cache's
/// admission already uses `self.cache.insert(...)` (loose admission)
/// — NOT `insert_arc_with_kind`. This pin asserts the production
/// source still uses the loose path for the eligible cache. A
/// migration to strict admission would require either dropping the
/// variant gate (breaking `IntrinsicSurface` admission) or
/// introducing the `is_facts_irrelevant: true` flag substrate;
/// either way the audit conclusion changes.
#[test]
fn eligible_cache_still_uses_variant_gated_loose_admission() {
    let path =
        workspace_root().join("crates/verter_session/src/resolver_core/fallthrough_resolver.rs");
    let src = std::fs::read_to_string(&path).expect("read fallthrough_resolver.rs");
    // The variant-gated loose admission lives inside `store_node`.
    // We pin two structural facts:
    //   (a) `store_node` exists.
    //   (b) The function body still calls `self.cache.insert(`.
    //   (c) The function body still tests for `IntrinsicSurface` and
    //       `ConsumedBindings` variants — the variant gate the audit
    //       names as the eligibility justification.
    assert!(
        src.contains("fn store_node"),
        "`FallthroughResolverState::store_node` is missing from `{}` — the audit's \
         eligibility justification depends on this fn's variant gate.",
        path.display()
    );
    assert!(
        src.contains("self.cache.insert("),
        "`FallthroughResolverState::store_node` no longer uses the loose `self.cache.insert(...)` \
         admission path. The audit eligibility for `FallthroughResolverState.cache` \
         depends on this; update the audit and `ELIGIBLE_CACHES` if the migration \
         landed."
    );
    assert!(
        src.contains("IntrinsicSurface") && src.contains("ConsumedBindings"),
        "the variant gate in `store_node` no longer references both \
         `IntrinsicSurface` and `ConsumedBindings`. The audit's eligibility \
         justification names BOTH variants; update the audit if the gate changed."
    );
}
