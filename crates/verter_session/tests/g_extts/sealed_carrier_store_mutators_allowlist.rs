//! Guard: `sealed_carrier_store_mutators_allowlist`.
//!
//! The single source-membership reconciler is the SOLE authority that mutates the
//! on-disk carrier-publish store. The low-level store mutators are sealed at the
//! module boundary (`pub(in crate::external_ts)`), but module visibility is too
//! COARSE for the real invariant: a NON-reconciler file that ALSO lives inside
//! `external_ts` (e.g. `tsserver_backend.rs`, `mod.rs`, a future sibling) would
//! still be allowed to call them by the compiler. This static guard is FINER than
//! the `pub(in external_ts)` seal: it AST-parses every `verter_lsp` production
//! source file and FAILS if a sealed store-mutator (or a re-introduced deleted
//! one) is CALLED from anywhere outside the authoritative allowlist.
//!
//! ## The on-disk store is mutated ONLY through these symbols
//!
//! - Two coordinator-level sealed mutators front the durable store —
//!   `retract_carrier` / `publish_owned_resolved`: the durable half of an
//!   authoritative membership transition; only the `DurableCarrierStore` impl
//!   block (the trait seam the reconciler drives) may call them.
//! - Two backend primitives do the actual manifest mutation —
//!   `retract_source_everywhere` / `retract_source_everywhere_except`: only the
//!   coordinator's sealed wrappers (in `publish_coordinator.rs`) and the backend
//!   itself (`tsserver_backend.rs`) may call them.
//! - Two names were DELETED with no shim and must never re-appear —
//!   `publish_carrier` / `reconcile_owner_loss`: forbidden as a CALL anywhere and
//!   as a re-introduced `fn` definition anywhere.
//!
//! `ProjectSync` is a PROVIDER-BUFFER abstraction (open/update/close in the engine
//! process); it does not mutate the on-disk carrier store, so the store-mutation
//! surface is exactly the six symbols above — guarding their call sites guards
//! every store mutation. The two legitimate provider-buffer resolvers
//! (`prepare_carrier_provider_sync_transition`, `carrier_sync_state_for_source`)
//! are deliberately NOT in the forbidden set.
//!
//! ## Why AST, not a text scan
//!
//! Exact-ident matching is load-bearing: live, legitimate methods share a PREFIX
//! with the forbidden names — `retract_carrier_from_external_ts` (the server-side
//! retract entry) and `publish_carrier_to_external_ts` (the drain publish entry)
//! must NOT trip the guard, while `retract_carrier` / `publish_carrier` MUST. A
//! `str::contains` scan cannot tell them apart; an AST method/path-segment ident
//! is compared for EQUALITY, so the prefix-sharing live names are clean.
//!
//! DISCRIMINATING: [`allowlist_self_test_discriminates`] feeds synthetic sources
//! and proves the scanner FIRES on a planted `retract_carrier(...)` call in a
//! non-allowlisted `external_ts` file, ACCEPTS the real `DurableCarrierStore` impl
//! call, and stays CLEAN on the prefix-sharing live names.

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

/// The SEALED carrier-sync GATEWAY: the SOLE file that may derive a carrier
/// provider state (`carrier_sync_state_for_source`) and mint the commit receipt.
const CARRIER_SYNC_GATEWAY: &str = "crates/verter_lsp/src/external_ts/carrier_sync.rs";

/// The sealed store-mutator names whose call sites are policed.
const SEALED_MUTATORS: &[&str] = &[
    "retract_carrier",
    "publish_owned_resolved",
    "retract_source_everywhere",
    "retract_source_everywhere_except",
];

/// The carrier-path RESOLVER that is now PRIVATE to the carrier-sync gateway. It
/// derives a carrier's owner-resolved `ProviderSyncState` (the IDE/API provider
/// paths); deriving that state is the FIRST half of the gap-E bug (a site that then
/// commits the buffer state while forgetting the membership publish/retract). It is
/// the language-level seal's static backstop: the only file that may compute carrier
/// provider paths is the gateway, so no site can commit carrier state — or return on
/// owner-loss — without routing the membership decision through
/// [`reconcile_carrier_source`].
const GATEWAY_RESOLVERS: &[&str] = &["carrier_sync_state_for_source"];

/// The carrier-companion ProjectSync CONTENT verbs (establish a carrier `.tsx`/`.dts`
/// companion as a provider content authority). They may run ONLY from the bounded
/// carrier-sync surface (the TGO direct-open handlers + the `ProjectSync` definition
/// itself) — never from a new unrelated file that would sync carrier buffers behind
/// the gateway's back. Close verbs are NOT policed (closing is always safe).
const CARRIER_CONTENT_VERBS: &[&str] = &["load_dts", "load_tsx", "open_dts", "open_tsx"];

/// Files permitted to call the carrier-companion CONTENT verbs: the carrier-sync
/// gateway, the TGO direct-open + interactive + drain + coordinator + scanner sync
/// sites (each holds a gateway receipt before committing), the open-document-liveness
/// preserve path, and the `ProjectSync` definition/its own inline tests.
const CONTENT_VERB_ALLOWLIST: &[&str] = &[
    CARRIER_SYNC_GATEWAY,
    "crates/verter_lsp/src/server/sync_orchestration.rs",
    "crates/verter_lsp/src/server/provider_state.rs",
    "crates/verter_lsp/src/background_drain.rs",
    "crates/verter_lsp/src/sync_coordinator.rs",
    "crates/verter_lsp/src/workspace_scanner.rs",
    "crates/verter_lsp/src/type_provider/project_sync.rs",
];

/// The DELETED names — forbidden as a call AND as a re-introduced `fn` definition,
/// anywhere (no allowlist entry). `prepare_carrier_provider_sync_transition` was the
/// server-side carrier transition resolver, deleted when its callers were routed
/// through the gateway; re-introducing it would re-open the off-gateway derive+commit
/// path.
const DELETED_NAMES: &[&str] = &[
    "publish_carrier",
    "reconcile_owner_loss",
    "prepare_carrier_provider_sync_transition",
];

/// Whether a call to `symbol` is permitted in `file_rel`, given whether the call
/// site is inside the `DurableCarrierStore` impl block.
fn call_is_allowed(symbol: &str, file_rel: &str, in_durable_impl: bool) -> bool {
    match symbol {
        // Coordinator-level mutators: ONLY the `DurableCarrierStore` impl block in
        // publish_coordinator.rs (the trait seam the reconciler drives). The
        // reconciler file is allowlisted defensively (it drives the trait, not
        // these inherent methods, today).
        "retract_carrier" | "publish_owned_resolved" => {
            (file_rel == PUBLISH_COORDINATOR && in_durable_impl)
                || file_rel == MEMBERSHIP_RECONCILER
        }
        // Backend primitives: the coordinator's sealed wrappers (publish_coordinator.rs)
        // and the backend's own defining file.
        "retract_source_everywhere" | "retract_source_everywhere_except" => {
            file_rel == PUBLISH_COORDINATOR
                || file_rel == TSSERVER_BACKEND
                || file_rel == MEMBERSHIP_RECONCILER
        }
        // Deleted names: never permitted (anywhere).
        _ if DELETED_NAMES.contains(&symbol) => false,
        // The carrier-path resolver: ONLY the carrier-sync gateway file.
        _ if GATEWAY_RESOLVERS.contains(&symbol) => file_rel == CARRIER_SYNC_GATEWAY,
        // Carrier-companion content verbs: only the bounded carrier-sync surface.
        _ if CARRIER_CONTENT_VERBS.contains(&symbol) => CONTENT_VERB_ALLOWLIST.contains(&file_rel),
        // Any non-policed symbol is allowed.
        _ => true,
    }
}

/// AST visitor: records every forbidden CALL site and every re-introduced DELETED
/// `fn` definition, tracking whether the cursor is inside the allowlisted
/// `DurableCarrierStore` impl block.
struct CallScanner {
    file_rel: String,
    in_durable_impl: bool,
    violations: Vec<String>,
}

impl CallScanner {
    /// Flag a forbidden call/reference by its exact method/path-segment ident.
    fn check_call(&mut self, name: &str) {
        let policed = SEALED_MUTATORS.contains(&name)
            || DELETED_NAMES.contains(&name)
            || GATEWAY_RESOLVERS.contains(&name)
            || CARRIER_CONTENT_VERBS.contains(&name);
        if policed && !call_is_allowed(name, &self.file_rel, self.in_durable_impl) {
            self.violations.push(format!(
                "{}: reference to sealed carrier symbol `{name}` outside the allowlist. \
                 Store mutators route through the `DurableCarrierStore` impl in \
                 {PUBLISH_COORDINATOR} (+ backend/reconciler); `carrier_sync_state_for_source` \
                 is PRIVATE to the carrier-sync gateway ({CARRIER_SYNC_GATEWAY}); the carrier \
                 content verbs run only from the bounded carrier-sync surface; the deleted names \
                 are forbidden everywhere. Route the carrier membership + provider-state commit \
                 through `reconcile_carrier_source` (the single gateway), never a private derive.",
                self.file_rel
            ));
        }
    }

    /// Flag a re-introduced DELETED `fn` definition by its exact ident.
    fn check_def(&mut self, name: &str) {
        if DELETED_NAMES.contains(&name) {
            self.violations.push(format!(
                "{}: definition of `fn {name}` re-introduces a DELETED store-mutator (it was \
                 removed with no shim). The membership transition routes through the reconciler.",
                self.file_rel
            ));
        }
    }
}

/// True iff `im` is the `impl DurableCarrierStore for CarrierPublishCoordinator`
/// block — the one trait seam the reconciler drives.
fn is_durable_carrier_store_impl(im: &syn::ItemImpl) -> bool {
    let trait_ok = im
        .trait_
        .as_ref()
        .and_then(|(_, path, _)| path.segments.last())
        .map(|seg| seg.ident == "DurableCarrierStore")
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
        let prev = self.in_durable_impl;
        // An impl block is the unit of context; nesting is handled by save/restore.
        self.in_durable_impl = is_durable_carrier_store_impl(im);
        syn::visit::visit_item_impl(self, im);
        self.in_durable_impl = prev;
    }

    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        self.check_call(&mc.method.to_string());
        syn::visit::visit_expr_method_call(self, mc);
    }

    /// Catch every PATH form by its last segment ident — a free-fn call
    /// (`carrier_sync_state_for_source(...)` whose func is a path), an associated
    /// path (`Type::sym`), AND a bare fn-pointer / path-VALUE reference
    /// (`let f = carrier_sync_state_for_source;` / passing it as a callback). A
    /// method call's receiver path is visited here too, but a method's own ident is
    /// NOT a path segment (it is handled by `visit_expr_method_call`), so there is no
    /// double counting. Hardened past call-only matching so a fn-pointer smuggle of a
    /// sealed resolver cannot bypass the guard.
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(seg) = path.segments.last() {
            self.check_call(&seg.ident.to_string());
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.check_def(&f.sig.ident.to_string());
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.check_def(&f.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, f);
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        self.check_def(&f.sig.ident.to_string());
        syn::visit::visit_trait_item_fn(self, f);
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
        in_durable_impl: false,
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

    // 2. The REAL allowlisted call (the DurableCarrierStore impl block in
    //    publish_coordinator.rs) is CLEAN.
    let real = scan_source(
        PUBLISH_COORDINATOR,
        "impl DurableCarrierStore for CarrierPublishCoordinator {\n\
         fn retract(&self, s: &str) -> R { self.retract_carrier(s) }\n\
         fn publish_owned(&self, b: &B, s: &str, c: &[C]) -> R { self.publish_owned_resolved(b, s, c) }\n\
         }",
    );
    assert!(
        real.is_empty(),
        "the DurableCarrierStore impl block's own calls must be allowed; got {real:?}"
    );

    // 2b. The SAME coordinator mutator called from publish_coordinator.rs but
    //     OUTSIDE the DurableCarrierStore impl block FIRES (block, not file,
    //     granularity).
    let outside_block = scan_source(
        PUBLISH_COORDINATOR,
        "impl CarrierPublishCoordinator { fn sneaky(&self, s: &str) { self.retract_carrier(s); } }",
    );
    assert!(
        outside_block.iter().any(|v| v.contains("retract_carrier")),
        "retract_carrier called outside the DurableCarrierStore impl block must fire even in \
         publish_coordinator.rs; got {outside_block:?}"
    );

    // 3. EXACT-IDENT discrimination: the prefix-sharing LIVE names are CLEAN
    //    everywhere.
    let live_prefix = scan_source(
        "crates/verter_lsp/src/server/lifecycle.rs",
        "async fn ok(s: &S, id: &str) {\n\
         s.retract_carrier_from_external_ts(id).await;\n\
         s.publish_carrier_to_external_ts(id).await;\n\
         s.reconcile_carrier_owner_loss_membership(id).await;\n\
         }",
    );
    assert!(
        live_prefix.is_empty(),
        "prefix-sharing LIVE methods (retract_carrier_from_external_ts / \
         publish_carrier_to_external_ts / reconcile_carrier_owner_loss_membership) must NOT trip \
         the exact-ident guard; got {live_prefix:?}"
    );

    // 3b. ...but the bare deleted names DO fire (call form).
    let deleted_call = scan_source(
        "crates/verter_lsp/src/server/lifecycle.rs",
        "async fn bad(s: &S, id: &str) { s.publish_carrier(id).await; s.reconcile_owner_loss(id); }",
    );
    assert_eq!(
        deleted_call.len(),
        2,
        "both deleted-name CALLS must fire; got {deleted_call:?}"
    );

    // 3c. ...and a re-introduced deleted `fn` DEFINITION fires.
    let deleted_def = scan_source(
        "crates/verter_lsp/src/external_ts/publish_coordinator.rs",
        "impl X { fn publish_carrier(&self) {} }",
    );
    assert!(
        deleted_def.iter().any(|v| v.contains("fn publish_carrier")),
        "a re-introduced `fn publish_carrier` definition must fire; got {deleted_def:?}"
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
}

/// DISCRIMINATING self-test for the carrier-sync GATEWAY seal: prove the guard
/// fires on the exact gap-E bypasses the structural fix closes — a site deriving
/// carrier provider state (and committing / returning on owner-loss) WITHOUT
/// routing through the single gateway — while accepting the gateway's own use.
#[test]
fn gateway_seal_self_test_discriminates() {
    // 1. THE gap-E BYPASS (workspace_scanner): the scanner derives the carrier
    //    provider state directly via the now-private resolver instead of the
    //    gateway, so it could commit the provider buffer while FORGETTING the
    //    membership publish/retract. FIRES.
    let scanner_bypass = scan_source(
        "crates/verter_lsp/src/workspace_scanner.rs",
        "async fn sync_file(r: &R, id: &str, j: bool) {\n\
         let next = crate::provider_sync::carrier_sync_state_for_source(r, id, j);\n\
         }",
    );
    assert!(
        scanner_bypass
            .iter()
            .any(|v| v.contains("carrier_sync_state_for_source")),
        "the workspace_scanner gap-E bypass (deriving carrier state off-gateway) must fire; \
         got {scanner_bypass:?}"
    );

    // 2. THE owner-loss `None => return` BYPASS: a site resolves the carrier state
    //    and, on owner-loss (`None`), returns WITHOUT retracting membership through
    //    the gateway. The `let Some(..) = carrier_sync_state_for_source(..) else {
    //    return; }` derive is itself off-gateway. FIRES.
    let owner_loss_bypass = scan_source(
        "crates/verter_lsp/src/sync_coordinator.rs",
        "async fn sync(r: &R, id: &str) {\n\
         let Some(next) = crate::provider_sync::carrier_sync_state_for_source(r, id, false) else {\n\
         return;\n\
         };\n\
         }",
    );
    assert!(
        owner_loss_bypass
            .iter()
            .any(|v| v.contains("carrier_sync_state_for_source")),
        "the owner-loss None=>return bypass (off-gateway derive) must fire; got {owner_loss_bypass:?}"
    );

    // 3. The GATEWAY's OWN use of its private resolver is CLEAN (it IS the gateway).
    let gateway_ok = scan_source(
        CARRIER_SYNC_GATEWAY,
        "fn close_target(r: &R, id: &str, j: bool) -> O { carrier_sync_state_for_source(r, id, j) }",
    );
    assert!(
        gateway_ok.is_empty(),
        "the carrier-sync gateway's own resolver call must be allowed; got {gateway_ok:?}"
    );

    // 4. visit_path HARDENING: a bare fn-POINTER / path-VALUE smuggle of the private
    //    resolver (not a direct call) from a non-gateway file FIRES — a callback
    //    smuggle cannot bypass a call-only matcher.
    let fn_pointer = scan_source(
        "crates/verter_lsp/src/server/sync_orchestration.rs",
        "fn smuggle() { let f: fn(&R, &str, bool) -> O = \
         crate::provider_sync::carrier_sync_state_for_source; let _ = f; }",
    );
    assert!(
        fn_pointer
            .iter()
            .any(|v| v.contains("carrier_sync_state_for_source")),
        "a fn-pointer reference to the private resolver from a non-gateway file must fire; \
         got {fn_pointer:?}"
    );

    // 5. The DELETED server resolver `prepare_carrier_provider_sync_transition` is
    //    forbidden as a CALL and as a re-introduced `fn` DEFINITION, anywhere.
    let deleted_call = scan_source(
        "crates/verter_lsp/src/server/lifecycle.rs",
        "async fn bad(s: &S, id: &str, j: bool) { s.prepare_carrier_provider_sync_transition(id, j); }",
    );
    assert!(
        deleted_call
            .iter()
            .any(|v| v.contains("prepare_carrier_provider_sync_transition")),
        "a call to the deleted prepare_carrier_provider_sync_transition must fire; got {deleted_call:?}"
    );
    let deleted_def = scan_source(
        "crates/verter_lsp/src/server/provider_state.rs",
        "impl S { fn prepare_carrier_provider_sync_transition(&self) {} }",
    );
    assert!(
        deleted_def
            .iter()
            .any(|v| v.contains("fn prepare_carrier_provider_sync_transition")),
        "a re-introduced prepare_carrier_provider_sync_transition definition must fire; \
         got {deleted_def:?}"
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

    // 7. NEGATIVE: the prefix-sharing live entry `carrier_close_state` (the close-only
    //    accessor) and the gateway public entry `reconcile_carrier_source` are NOT
    //    policed — they are the SANCTIONED surface and must stay clean everywhere.
    let sanctioned = scan_source(
        "crates/verter_lsp/src/server/lifecycle.rs",
        "async fn ok(s: &S, id: &str, j: bool) {\n\
         let _ = s.carrier_close_state(id, j);\n\
         let _ = crate::external_ts::reconcile_carrier_source(req).await;\n\
         }",
    );
    assert!(
        sanctioned.is_empty(),
        "the sanctioned carrier_close_state / reconcile_carrier_source surface must stay clean; \
         got {sanctioned:?}"
    );
}
