//! Call-site guard: the PRIMARY carrier provider-state admission + non-owned disposition
//! shapes route through the SINGLE `CarrierTransactionCoordinator`.
//!
//! This AUDITS the primary install/drop shapes of the carrier-sync race cluster: it
//! enumerates the textual `ProviderSyncState` admission-token installer shapes and the
//! reconcile-outcome consumers, and flags a raw carrier commit or a hand-rolled non-owned
//! disposition outside the coordinator (its body + the tracked declaration-closure
//! exemption). It is NOT a full class-closure: remaining bypass shapes — a named or discarded
//! `AdmitOutcome`, a consumed `Superseded` that omits its requeue, and generic struct-literal
//! installs on non-primary paths (a `..spread` or a helper-returned pre-stamped state) — stay
//! REVIEW-AUDITED pending the dedicated carrier-sync-concurrency hardening block. It catches
//! the simple dropped-outcome / vacant-resurrection / equal-key reintroductions the primary
//! predicates below cover, not every reintroduction of the class.
//!
//! Five predicates (primary-shape coverage):
//!   1. The admission token (`committed_ide_surface` / `commit_stamp`) is INSTALLED only in
//!      the coordinator body — the primary "installer" shape (narrowed for F6: the
//!      declaration overlay mutates ONLY the `Decl` kind, never the admission token). This
//!      covers the assignment form and the struct-literal `Some(..)` field-init form (a
//!      generic owned-state commit by construction); a `..spread` or a helper-returned
//!      pre-stamped state is a non-primary shape this textual predicate does not see.
//!   2. The removed raw free `commit_carrier_provider_state` never re-appears — the primary
//!      carrier commit path is `CarrierTransactionCoordinator::admit_owned`.
//!   3. Every file that consumes the opaque non-owned arm (`CarrierSyncDecision::NotOwned`)
//!      is enumerated AND routes it through the coordinator's `settle`.
//!   4. A carrier-sync gateway decision is never `let _ =`-dropped (its `#[must_use]` catches
//!      a bare-statement drop; the guard catches the explicit `let _ =` suppression).
//!   5. An `admit_owned` outcome is never `let _ =`-dropped — the `#[must_use]` `AdmitOutcome`
//!      forces consumption. Whether a consumed `Superseded` actually requeues is NOT
//!      statically enforced here: the primary interactive paths requeue, but a named or
//!      discarded outcome, or a consumed `Superseded` that omits its requeue, stays
//!      review-audited pending the carrier-sync-concurrency hardening block.

use std::fs;
use std::path::{Path, PathBuf};

/// The `crates/verter_lsp/src` root, from this test crate's manifest dir.
fn lsp_src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("crates")
        .join("verter_lsp")
        .join("src")
}

fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Production (non-test) `verter_lsp` source files.
fn production_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_rs(&lsp_src_root(), &mut files);
    files
        .into_iter()
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            !name.ends_with("_tests.rs")
        })
        .collect()
}

/// Drop `//` line comments so a doc/comment reference is not a false positive.
fn strip_line(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Whether `path` ends with the given `crates/verter_lsp/src`-relative suffix.
fn rel_ends_with(path: &Path, suffix: &str) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.ends_with(suffix)
}

/// The coordinator body — the SOLE admission-token installer + the definer of `settle`.
const COORDINATOR: &str = "external_ts/carrier_sync.rs";
/// The tracked declaration-closure exemption: it mutates ONLY the `Decl` kind / a fresh
/// decl-only state, never the receipt-attested IDE stamp / commit stamp (F6).
const DECL_CLOSURE_EXEMPTION: &str = "background_drain_decl_closure.rs";

/// Every production site that names the non-owned gateway arm `CarrierSyncDecision::NotOwned`.
/// A NEW site must be added here AND route the outcome through the coordinator's `settle`.
const NON_OWNED_SITES: &[&str] = &[
    // The coordinator: defines the arm + `settle`.
    "external_ts/carrier_sync.rs",
    // The `&self` gateway wrapper: PRODUCES the bootstrap `NotOwned` (returned `#[must_use]`,
    // so its caller must settle) and routes owned commits through `admit_owned`.
    "server/provider_state.rs",
    // The consumers — each settles the non-owned outcome.
    "server/sync_orchestration.rs",
    "sync_coordinator.rs",
    "workspace_scanner.rs",
    "background_drain.rs",
    "background_drain_owner_loss.rs",
];

/// The non-owned CONSUMERS (a subset of [`NON_OWNED_SITES`] excluding the coordinator body
/// and the producer wrapper) — each MUST route the outcome through the coordinator `settle`.
const NON_OWNED_CONSUMERS: &[&str] = &[
    "server/sync_orchestration.rs",
    "sync_coordinator.rs",
    "workspace_scanner.rs",
    "background_drain.rs",
    "background_drain_owner_loss.rs",
];

#[test]
fn admission_token_installed_only_by_the_coordinator() {
    // The receipt-attested IDE-surface stamp + commit stamp (the admission token) are
    // ASSIGNED only inside the coordinator body (`admit_owned`). A declaration-overlay
    // mutation touches ONLY `decl_path`, never these fields (the narrowed F6 wording).
    let mut violations = Vec::new();
    for path in production_files() {
        if rel_ends_with(&path, COORDINATOR) {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read src");
        for (n, line) in src.lines().enumerate() {
            let code = strip_line(line);
            if code.contains(".committed_ide_surface =") || code.contains(".commit_stamp =") {
                violations.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "the admission token (committed_ide_surface / commit_stamp) must be installed ONLY by \
         the coordinator (external_ts/carrier_sync.rs::admit_owned); found an assignment \
         outside it — route the carrier commit through CarrierTransactionCoordinator::admit_owned: \
         {violations:?}"
    );
}

#[test]
fn no_raw_carrier_commit_free_fn_reintroduced() {
    // The removed raw free `commit_carrier_provider_state` must never re-appear as a
    // qualified call — the SOLE carrier commit path is the coordinator's `admit_owned`.
    let mut violations = Vec::new();
    for path in production_files() {
        let src = fs::read_to_string(&path).expect("read src");
        for (n, line) in src.lines().enumerate() {
            let code = strip_line(line);
            if code.contains("external_ts::commit_carrier_provider_state(") {
                violations.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "the raw carrier-commit free fn was re-introduced — route through \
         CarrierTransactionCoordinator::admit_owned instead: {violations:?}"
    );
}

#[test]
fn every_non_owned_consumer_is_enumerated_and_routes_through_settle() {
    // (a) The set of production files naming `CarrierSyncDecision::NotOwned` must EQUAL the
    //     enumerated allowlist — a new consumer forces a conscious registration here.
    // (b) Every enumerated CONSUMER (not the coordinator body / not the producer wrapper)
    //     must route the outcome through the coordinator `settle` — never handle/drop it raw.
    let mut found: Vec<String> = Vec::new();
    for path in production_files() {
        let src = fs::read_to_string(&path).expect("read src");
        let has_non_owned = src
            .lines()
            .any(|line| strip_line(line).contains("CarrierSyncDecision::NotOwned"));
        if has_non_owned {
            let rel = path.to_string_lossy().replace('\\', "/");
            let suffix = NON_OWNED_SITES
                .iter()
                .find(|s| rel.ends_with(*s))
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("UNENUMERATED::{rel}"));
            found.push(suffix);
        }
    }
    found.sort();
    found.dedup();
    let mut expected: Vec<String> = NON_OWNED_SITES.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "the set of `CarrierSyncDecision::NotOwned` sites drifted from the enumerated \
         allowlist — a NEW consumer must be registered in NON_OWNED_SITES and route through \
         CarrierTransactionCoordinator::settle"
    );

    // (b) each consumer routes through `settle`.
    let mut missing_settle = Vec::new();
    for path in production_files() {
        let rel = path.to_string_lossy().replace('\\', "/");
        if !NON_OWNED_CONSUMERS.iter().any(|s| rel.ends_with(*s)) {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read src");
        if !src.contains(".settle(") {
            missing_settle.push(rel);
        }
    }
    assert!(
        missing_settle.is_empty(),
        "a non-owned carrier consumer does not route through the coordinator `settle`: \
         {missing_settle:?}"
    );
}

#[test]
fn carrier_gateway_decision_is_never_let_underscore_dropped() {
    // A carrier-sync gateway call must have its `CarrierSyncDecision` consumed — a `let _ =`
    // drop would suppress the `#[must_use]` and silently lose the non-owned requeue /
    // owner-loss barrier advance (the F4 dropped-outcome finding). The enum's `#[must_use]`
    // already catches a bare-statement drop; this catches the explicit `let _ =` suppression.
    let mut violations = Vec::new();
    for path in production_files() {
        let src = fs::read_to_string(&path).expect("read src");
        for (n, line) in src.lines().enumerate() {
            let code = strip_line(line);
            let dropped = code.contains("let _ =")
                && (code.contains("reconcile_carrier_source")
                    || code.contains("reconcile_carrier_via_gateway"));
            if dropped {
                violations.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "a carrier-sync gateway decision is `let _ =`-dropped — consume it (match the owned \
         arms + settle the CarrierNotOwned, or into_owned_commit_authorization): {violations:?}"
    );
}

#[test]
fn owned_admission_token_never_struct_literal_installed_outside_coordinator() {
    // The admission token (`commit_stamp` / `committed_ide_surface`) reaches a NON-NONE value
    // ONLY through the coordinator. This closes the STRUCT-LITERAL install bypass — a
    // `ProviderSyncState { commit_stamp: Some(..), .. }` /
    // `{ committed_ide_surface: Some(..), .. }` field init that would install the token by
    // construction (a generic owned-state commit), the form the sibling
    // `admission_token_installed_only_by_the_coordinator` assignment predicate does not see. A
    // `commit_stamp: None` / `committed_ide_surface: None` field init (the normal construction)
    // is NOT a bypass.
    let mut violations = Vec::new();
    for path in production_files() {
        if rel_ends_with(&path, COORDINATOR) {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read src");
        for (n, line) in src.lines().enumerate() {
            let code = strip_line(line);
            if code.contains("commit_stamp: Some(") || code.contains("committed_ide_surface: Some(")
            {
                violations.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "the admission token (commit_stamp / committed_ide_surface) must be installed with a \
         non-None value ONLY by the coordinator (external_ts/carrier_sync.rs::admit_owned); a \
         struct-literal `Some(..)` field init installs it by construction — route the carrier \
         commit through CarrierTransactionCoordinator::admit_owned: {violations:?}"
    );
}

#[test]
fn admit_outcome_is_never_let_underscore_dropped() {
    // Every `admit_owned` outcome is CONSUMED: a `let _ = ..admit_owned(..)` suppresses the
    // `#[must_use]` `AdmitOutcome` and silently drops the requeue-on-`Superseded` obligation
    // (a newer transaction reclaimed the source / an owner-loss advanced the barrier) — the
    // F3/F4 dropped-outcome class. The consumed form
    // (`if ..admit_owned(..) == Superseded { .. }`) is NOT flagged.
    let mut violations = Vec::new();
    for path in production_files() {
        let src = fs::read_to_string(&path).expect("read src");
        for (n, line) in src.lines().enumerate() {
            let code = strip_line(line);
            if code.contains("let _ =") && code.contains("admit_owned(") {
                violations.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "an admit_owned outcome is `let _ =`-dropped — consume the AdmitOutcome (requeue on \
         Superseded, gate any stale-path close on Admitted): {violations:?}"
    );
}

// ---------------------------------------------------------------------------
// The guard predicates DISCRIMINATE — a planted raw bypass is caught (RED), and
// the coordinator-routed form is not (GREEN).
// ---------------------------------------------------------------------------

#[test]
fn guard_predicates_discriminate_a_planted_bypass() {
    // Token-installer predicate: a raw `state.commit_stamp = ...` bypass is caught, while a
    // struct-literal `commit_stamp: None` field init (the ProviderSyncState construction that
    // is not an install) is NOT.
    let bypass = "        state.commit_stamp = Some(incoming);";
    let field_init = "        commit_stamp: None,";
    assert!(strip_line(bypass).contains(".commit_stamp ="));
    assert!(!strip_line(field_init).contains(".commit_stamp ="));

    // Raw-free-fn predicate: the removed call is caught; the `&self` wrapper / method call is
    // not (it routes to admit_owned).
    let raw = "        crate::external_ts::commit_carrier_provider_state(&states, id, s, &r);";
    let wrapper = "        self.commit_carrier_provider_state(id, s, &r);";
    assert!(strip_line(raw).contains("external_ts::commit_carrier_provider_state("));
    assert!(!strip_line(wrapper).contains("external_ts::commit_carrier_provider_state("));

    // Comment references are stripped (not false positives).
    let comment = "        // external_ts::commit_carrier_provider_state was removed";
    assert!(!strip_line(comment).contains("external_ts::commit_carrier_provider_state("));

    // Struct-literal token-install predicate: a `Some(..)` field init (a generic owned-state
    // commit by construction) is caught; the `None` field init (the normal construction) is
    // not — for BOTH admission-token fields.
    let stamp_bypass = "            commit_stamp: Some(incoming),";
    let stamp_ok = "            commit_stamp: None,";
    let surface_bypass = "            committed_ide_surface: Some(stamp),";
    let surface_ok = "            committed_ide_surface: None,";
    assert!(strip_line(stamp_bypass).contains("commit_stamp: Some("));
    assert!(!strip_line(stamp_ok).contains("commit_stamp: Some("));
    assert!(strip_line(surface_bypass).contains("committed_ide_surface: Some("));
    assert!(!strip_line(surface_ok).contains("committed_ide_surface: Some("));

    // Dropped-admit-outcome predicate: a `let _ = ..admit_owned(..)` is caught; the consumed
    // `if ..admit_owned(..) == Superseded` form is not.
    let admit_drop = "        let _ = coord.admit_owned(&states, id, state, &receipt);";
    let admit_ok = "        if coord.admit_owned(&states, id, state, &receipt) == Superseded {";
    assert!(
        strip_line(admit_drop).contains("let _ =")
            && strip_line(admit_drop).contains("admit_owned(")
    );
    assert!(
        !(strip_line(admit_ok).contains("let _ =")
            && strip_line(admit_ok).contains("admit_owned("))
    );
}

#[test]
fn guard_allowlist_paths_exist() {
    // A stale allowlist entry (a renamed/removed file) is itself a guard rot — every
    // enumerated path must exist so the enumeration stays load-bearing.
    for suffix in NON_OWNED_SITES {
        let path = lsp_src_root().join(suffix);
        assert!(
            path.exists(),
            "enumerated non-owned site {suffix} does not exist — re-point the guard"
        );
    }
    assert!(
        lsp_src_root().join(DECL_CLOSURE_EXEMPTION).exists(),
        "the tracked declaration-closure exemption file does not exist — re-point the guard"
    );
}
