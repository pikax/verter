//! Guard: `sealed_carrier_store_mutators_allowlist`.
//!
//! The single source-membership reconciler is the SOLE authority that mutates the
//! on-disk carrier-publish store. The low-level store mutators are sealed at the
//! module boundary (`pub(in crate::external_ts)`), but module visibility is too
//! COARSE for the real invariant: a NON-reconciler file that ALSO lives inside
//! `external_ts` (e.g. `tsserver_backend.rs`, `mod.rs`, a future sibling) would
//! still be allowed to call them by the compiler. This static guard is FINER than
//! the `pub(in external_ts)` seal: it AST-parses every `verter_lsp` production
//! source file and FAILS if a sealed store-mutator is CALLED from anywhere
//! outside the authoritative allowlist.
//!
//! ## The on-disk store is mutated ONLY through these symbols
//!
//! - Two coordinator-level sealed mutators front the durable store —
//!   `retract_carrier` / `publish_owned_resolved`: the durable half of an
//!   authoritative membership transition; only the `CarrierMembershipCommitter` impl
//!   block (the trait seam the reconciler drives) may call them.
//! - Two backend primitives do the actual manifest mutation —
//!   `retract_source_everywhere` / `retract_source_everywhere_except`: only the
//!   coordinator's sealed wrappers (in `publish_coordinator.rs`) and the backend
//!   itself (`tsserver_backend.rs`) may call them.
//!
//! `ProjectSync` is a PROVIDER-BUFFER abstraction (open/update/close in the engine
//! process); it does not mutate the on-disk carrier store, so the store-mutation
//! surface is exactly the four symbols above — guarding their call sites guards
//! every store mutation. The carrier-companion CONTENT verbs are policed separately:
//! they may run only from the bounded carrier-sync surface.
//!
//! ## Why this scan is still the only check of the invariant
//!
//! Every other invariant this directory once scanned for has a real rail that
//! subsumes it: the bound-project witness is a compiler-enforced type-state, the
//! non-owning attach surface has no raw-wire accessor to reach in the first
//! place, the gated write channel refuses on live wire traffic, and the shared
//! serve-mode decision fails closed per missing provenance fact under its own
//! behavioral tests. This one does not. The language seal
//! (`pub(in crate::external_ts)`) stops a caller OUTSIDE the module, but the
//! failure mode here is a caller INSIDE it — a sibling `external_ts` file that
//! commits carrier buffer state while forgetting the membership publish/retract.
//! Making that unrepresentable needs a capability the store mutators demand and
//! only the reconciler can mint; until that exists, deleting this scan would
//! leave the invariant with no check at all.
//!
//! ## Why AST, not a text scan
//!
//! Exact-ident matching is load-bearing: live, legitimate methods share a PREFIX
//! with the policed names — `retract_carrier_from_external_ts` (the server-side
//! retract entry) must NOT trip the guard, while `retract_carrier` MUST. A
//! `str::contains` scan cannot tell them apart; an AST method/path-segment ident
//! is compared for EQUALITY, so the prefix-sharing live names are clean.
//!
//! DISCRIMINATING: [`allowlist_self_test_discriminates`] feeds synthetic sources
//! and proves the scanner FIRES on a planted `retract_carrier(...)` call in a
//! non-allowlisted `external_ts` file, ACCEPTS the real `CarrierMembershipCommitter`
//! impl call, and stays CLEAN on the prefix-sharing live names.

use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

/// Repo root (two parents up from `crates/verter_session`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

// ── Allowlisted file identities (forward-slash normalised, cross-platform) ──
const PUBLISH_COORDINATOR: &str = "crates/verter_lsp/src/external_ts/publish_coordinator.rs";
const TSSERVER_BACKEND: &str = "crates/verter_lsp/src/external_ts/tsserver_backend.rs";
const MEMBERSHIP_RECONCILER: &str = "crates/verter_lsp/src/external_ts/membership_reconciler.rs";

/// The carrier-sync gateway: the single file that reconciles a carrier's membership
/// and provider state together.
const CARRIER_SYNC_GATEWAY: &str = "crates/verter_lsp/src/external_ts/carrier_sync.rs";

/// The sealed store-mutator names whose call sites are policed.
const SEALED_MUTATORS: &[&str] = &[
    "retract_carrier",
    "publish_owned_resolved",
    "retract_source_everywhere",
    "retract_source_everywhere_except",
];

/// The carrier-companion ProjectSync CONTENT verbs (establish a carrier `.tsx`/`.dts`
/// companion as a provider content authority). They may run ONLY from the bounded
/// carrier-sync surface (the tsgo direct-open handlers + the `ProjectSync` definition
/// itself) — never from a new unrelated file that would sync carrier buffers behind
/// the gateway's back. Close verbs are NOT policed (closing is always safe).
const CARRIER_CONTENT_VERBS: &[&str] = &["load_dts", "load_tsx", "open_dts", "open_tsx"];

/// Files permitted to call the carrier-companion CONTENT verbs: the carrier-sync
/// gateway, the tsgo direct-open + interactive + drain + coordinator + scanner sync
/// sites (each holds a gateway receipt before committing), the open-document-liveness
/// preserve path, and the `ProjectSync` definition/its own inline tests.
const CONTENT_VERB_ALLOWLIST: &[&str] = &[
    CARRIER_SYNC_GATEWAY,
    "crates/verter_lsp/src/server/sync_orchestration.rs",
    "crates/verter_lsp/src/server/provider_state.rs",
    "crates/verter_lsp/src/background_drain.rs",
    // The proactive declaration-overlay closure pass — a drain carrier-sync site
    // kept separate from `background_drain.rs` so closure discovery stays focused. It
    // opens each carrier dependency's `.d.<ext>.ts` declaration companion via the
    // same bounded content verbs the drain uses for the IDE/API companions; it is
    // not a store-membership mutator and routes no off-gateway commit.
    "crates/verter_lsp/src/background_drain_decl_closure.rs",
    "crates/verter_lsp/src/sync_coordinator.rs",
    "crates/verter_lsp/src/workspace_scanner.rs",
    "crates/verter_lsp/src/type_provider/project_sync.rs",
];

/// Whether a call to `symbol` is permitted in `file_rel`, given whether the call
/// site is inside the `CarrierMembershipCommitter` impl block.
fn call_is_allowed(symbol: &str, file_rel: &str, in_committer_impl: bool) -> bool {
    match symbol {
        // Coordinator-level mutators: ONLY the `CarrierMembershipCommitter` impl block
        // in publish_coordinator.rs (the trait seam the reconciler drives). The
        // reconciler file is allowlisted defensively (it drives the trait, not
        // these inherent methods, today).
        "retract_carrier" | "publish_owned_resolved" => {
            (file_rel == PUBLISH_COORDINATOR && in_committer_impl)
                || file_rel == MEMBERSHIP_RECONCILER
        }
        // Backend primitives: the coordinator's sealed wrappers (publish_coordinator.rs)
        // and the backend's own defining file.
        "retract_source_everywhere" | "retract_source_everywhere_except" => {
            file_rel == PUBLISH_COORDINATOR
                || file_rel == TSSERVER_BACKEND
                || file_rel == MEMBERSHIP_RECONCILER
        }
        // Carrier-companion content verbs: only the bounded carrier-sync surface.
        _ if CARRIER_CONTENT_VERBS.contains(&symbol) => CONTENT_VERB_ALLOWLIST.contains(&file_rel),
        // Any non-policed symbol is allowed.
        _ => true,
    }
}

/// AST visitor: records every forbidden CALL site, tracking whether the cursor is
/// inside the allowlisted `CarrierMembershipCommitter` impl block.
struct CallScanner {
    file_rel: String,
    in_committer_impl: bool,
    violations: Vec<String>,
}

impl CallScanner {
    /// Flag a forbidden call/reference by its exact method/path-segment ident.
    fn check_call(&mut self, name: &str) {
        let policed = SEALED_MUTATORS.contains(&name) || CARRIER_CONTENT_VERBS.contains(&name);
        if policed && !call_is_allowed(name, &self.file_rel, self.in_committer_impl) {
            self.violations.push(format!(
                "{}: reference to sealed carrier symbol `{name}` outside the allowlist. \
                 Store mutators route through the `CarrierMembershipCommitter` impl in \
                 {PUBLISH_COORDINATOR} (+ backend/reconciler); the carrier content verbs run \
                 only from the bounded carrier-sync surface. Route the carrier membership + \
                 provider-state commit through `reconcile_carrier_source` (the single gateway).",
                self.file_rel
            ));
        }
    }
}

/// True iff `im` is the `impl CarrierMembershipCommitter for CarrierPublishCoordinator`
/// block — the one trait seam the reconciler drives.
fn is_carrier_membership_committer_impl(im: &syn::ItemImpl) -> bool {
    let trait_ok = im
        .trait_
        .as_ref()
        .and_then(|(_, path, _)| path.segments.last())
        .map(|seg| seg.ident == "CarrierMembershipCommitter")
        .unwrap_or(false);
    let self_ok = match &*im.self_ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|seg| seg.ident == "CarrierPublishCoordinator")
            .unwrap_or(false),
        _ => false,
    };
    trait_ok && self_ok
}

impl<'ast> Visit<'ast> for CallScanner {
    fn visit_item_impl(&mut self, im: &'ast syn::ItemImpl) {
        let prev = self.in_committer_impl;
        // An impl block is the unit of context; nesting is handled by save/restore.
        self.in_committer_impl = is_carrier_membership_committer_impl(im);
        syn::visit::visit_item_impl(self, im);
        self.in_committer_impl = prev;
    }

    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        self.check_call(&mc.method.to_string());
        syn::visit::visit_expr_method_call(self, mc);
    }

    /// Catch every PATH form by its last segment ident — a free-fn call whose func
    /// is a path, an associated path (`Type::sym`), AND a bare fn-pointer /
    /// path-VALUE reference (`let f = Backend::retract_source_everywhere;` / passing
    /// it as a callback). A method call's receiver path is visited here too, but a
    /// method's own ident is NOT a path segment (it is handled by
    /// `visit_expr_method_call`), so there is no double counting.
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(seg) = path.segments.last() {
            self.check_call(&seg.ident.to_string());
        }
        syn::visit::visit_path(self, path);
    }
}

/// Scan one source string as if it were `file_rel`, returning the violations.
/// Factored out so the self-test can feed synthetic inputs.
fn scan_source(file_rel: &str, src: &str) -> Vec<String> {
    let Ok(file) = syn::parse_file(src) else {
        // An unparseable production file cannot be checked here; the workspace
        // compile gate already requires it to parse. Treat as no violations (the
        // real test below only feeds genuine source).
        return Vec::new();
    };
    let mut scanner = CallScanner {
        file_rel: file_rel.to_string(),
        in_committer_impl: false,
        violations: Vec::new(),
    };
    scanner.visit_file(&file);
    scanner.violations
}

/// Recursively collect every `.rs` file under `path` whose name does NOT end with
/// `_tests.rs` (a test file legitimately constructs/exercises the sealed mutators
/// through their public seam).
fn production_rs_files(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            production_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            let is_test = p
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with("_tests.rs"))
                .unwrap_or(false);
            if !is_test {
                out.push(p);
            }
        }
    }
}

/// Repo-relative, forward-slash-normalised path for a source file.
fn rel_forward_slash(file: &Path) -> String {
    file.strip_prefix(workspace_root())
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn sealed_carrier_store_mutators_allowlist() {
    let src_dir = workspace_root()
        .join("crates")
        .join("verter_lsp")
        .join("src");
    let mut files = Vec::new();
    production_rs_files(&src_dir, &mut files);
    assert!(
        files.len() > 20,
        "expected to scan the verter_lsp production source tree; found only {} files (path wrong?)",
        files.len()
    );

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        let Ok(src) = fs::read_to_string(file) else {
            continue;
        };
        violations.extend(scan_source(&rel_forward_slash(file), &src));
    }

    assert!(
        violations.is_empty(),
        "sealed carrier-store mutator(s) called/redefined outside the reconciler allowlist:\n  - {}",
        violations.join("\n  - ")
    );
}

/// DISCRIMINATING self-test: feed synthetic sources and prove the scanner fires
/// on the exact bypass the brief plants, accepts the real allowlisted call, and
/// stays clean on the prefix-sharing LIVE names (exact-ident discrimination).
#[test]
fn allowlist_self_test_discriminates() {
    // 1. A planted `retract_carrier(...)` call in a NON-allowlisted external_ts
    //    file FIRES (the brief's red proof, AST form).
    let planted = scan_source(
        "crates/verter_lsp/src/external_ts/mod.rs",
        "fn rogue(c: &Coord) { c.retract_carrier(\"/a.vue\"); }",
    );
    assert!(
        planted.iter().any(|v| v.contains("retract_carrier")),
        "a retract_carrier call outside the allowlist must trip the guard; got {planted:?}"
    );

    // 2. The REAL allowlisted call (the CarrierMembershipCommitter impl block in
    //    publish_coordinator.rs) is CLEAN.
    let real = scan_source(
        PUBLISH_COORDINATOR,
        "impl CarrierMembershipCommitter for CarrierPublishCoordinator {\n\
         fn retract<'a>(&'a self, s: &'a str) -> F { Box::pin(async move { self.retract_carrier(s) }) }\n\
         fn commit_owned<'a>(&'a self, b: &'a B, s: &'a str, c: &'a [C]) -> F { Box::pin(async move { self.publish_owned_resolved(b, s, c) }) }\n\
         }",
    );
    assert!(
        real.is_empty(),
        "the CarrierMembershipCommitter impl block's own calls must be allowed; got {real:?}"
    );

    // 2b. The SAME coordinator mutator called from publish_coordinator.rs but
    //     OUTSIDE the CarrierMembershipCommitter impl block FIRES (block, not file,
    //     granularity).
    let outside_block = scan_source(
        PUBLISH_COORDINATOR,
        "impl CarrierPublishCoordinator { fn sneaky(&self, s: &str) { self.retract_carrier(s); } }",
    );
    assert!(
        outside_block.iter().any(|v| v.contains("retract_carrier")),
        "retract_carrier called outside the CarrierMembershipCommitter impl block must fire even in \
         publish_coordinator.rs; got {outside_block:?}"
    );

    // 3. EXACT-IDENT discrimination: the prefix-sharing LIVE name is CLEAN
    //    everywhere.
    let live_prefix = scan_source(
        "crates/verter_lsp/src/server/lifecycle.rs",
        "async fn ok(s: &S, id: &str) { s.retract_carrier_from_external_ts(id).await; }",
    );
    assert!(
        live_prefix.is_empty(),
        "the prefix-sharing LIVE method retract_carrier_from_external_ts must NOT trip the \
         exact-ident guard; got {live_prefix:?}"
    );

    // 4. A backend primitive is allowed in publish_coordinator.rs (the sealed
    //    wrapper methods) but FIRES from a non-allowlisted file.
    let backend_ok = scan_source(
        PUBLISH_COORDINATOR,
        "impl C { fn retract_carrier(&self, s: &str) { self.backend.retract_source_everywhere(s); } }",
    );
    assert!(
        backend_ok.is_empty(),
        "retract_source_everywhere in the coordinator's sealed wrapper must be allowed; got {backend_ok:?}"
    );
    let backend_bad = scan_source(
        "crates/verter_lsp/src/external_ts/mod.rs",
        "fn rogue(b: &B, s: &str) { b.retract_source_everywhere_except(s, \"/p\"); }",
    );
    assert!(
        backend_bad
            .iter()
            .any(|v| v.contains("retract_source_everywhere_except")),
        "a backend-primitive call from a non-allowlisted file must fire; got {backend_bad:?}"
    );

    // 5. visit_path HARDENING: a bare fn-POINTER / path-VALUE smuggle of a backend
    //    primitive (not a method call) from a non-allowlisted file FIRES.
    let fn_pointer = scan_source(
        "crates/verter_lsp/src/server/sync_orchestration.rs",
        "fn smuggle() { let f = TsserverBackend::retract_source_everywhere; let _ = f; }",
    );
    assert!(
        fn_pointer
            .iter()
            .any(|v| v.contains("retract_source_everywhere")),
        "a fn-pointer reference to a backend primitive from a non-allowlisted file must fire; \
         got {fn_pointer:?}"
    );

    // 6. Carrier CONTENT verbs: a carrier-buffer open from a NON-allowlisted file
    //    FIRES; the same verb from an allowlisted carrier-sync site is CLEAN.
    let content_bypass = scan_source(
        "crates/verter_lsp/src/server/nav_features.rs",
        "async fn rogue(sync: &P, p: &str, c: &str) { let _ = sync.open_tsx(p, c).await; }",
    );
    assert!(
        content_bypass.iter().any(|v| v.contains("open_tsx")),
        "a carrier content verb from a non-allowlisted file must fire; got {content_bypass:?}"
    );
    let content_ok = scan_source(
        "crates/verter_lsp/src/background_drain.rs",
        "async fn ok(sync: &P, p: &str, c: &str) { let _ = sync.open_tsx(p, c).await; }",
    );
    assert!(
        content_ok.is_empty(),
        "a carrier content verb from an allowlisted carrier-sync file must be clean; got {content_ok:?}"
    );
}
