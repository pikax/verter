//! Guard: `ledger_is_off_the_serve_path`.
//!
//! The in-process [`MembershipLedger`] is INTERNAL transition bookkeeping ONLY (the
//! reconciler's `current_session` / `record_snapshot` reads + commit verification;
//! its sole writer is the reconciler's `commit`). Live `getExternalFiles` is served
//! CROSS-PROCESS from the on-disk store `ready_files` (the Node plugin reads the
//! `carrier_publish_store` manifest), NOT this ledger. Until the ledger's read path is
//! wired, NO production (non-`#[cfg(test)]`) serve / `getExternalFiles` path may read
//! the ledger's advertised set — otherwise a half-wired read path could diverge from
//! the cross-process store the plugin actually serves.
//!
//! This guard AST-parses every `verter_lsp` production source file and FAILS if a
//! ledger advertised/serve-set reader is CALLED from production code:
//!
//! - `external_files_for_project` (the backend's ledger accessor) has NO production
//!   caller — its only callers today are `#[cfg(test)]` (the workspace-scanner
//!   inline test mod and the `#[cfg(test)]` `external_ts_advertised_for_project`
//!   server accessor). A production caller would put the ledger on the serve path.
//! - `advertised_provider_paths_under` (the `MembershipLedger` primitive) is read
//!   ONLY by `external_files_for_project`'s own body in `tsserver_backend.rs` (which
//!   is itself off the serve path, by the rule above) and by its defining file —
//!   every OTHER production reader is a direct ledger serve-read and FAILS.
//!
//! ## Why AST + `#[cfg(test)]` ancestry, not a text scan
//!
//! The two test callers live INSIDE production files (`workspace_scanner.rs`'s inline
//! `#[cfg(test)] mod tests`, `sync_orchestration.rs`'s `#[cfg(test)]`
//! `external_ts_advertised_for_project`), so a basename `_tests.rs` skip cannot see
//! them. The scanner tracks `#[cfg(test)]` ancestry over `mod` / `fn` / `impl` items
//! and skips any call under a `#[cfg(test)]` gate, so it polices PRODUCTION calls
//! exactly — the ledger's existence is fine, only a production serve-read is the
//! violation.
//!
//! DISCRIMINATING: [`serve_path_self_test_discriminates`] plants a production
//! (non-`cfg(test)`) caller of each policed symbol and proves the scanner FIRES,
//! plants a `#[cfg(test)]` caller and proves it is SKIPPED, and confirms the
//! allowlisted `external_files_for_project` accessor body stays CLEAN.

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
const TSSERVER_BACKEND: &str = "crates/verter_lsp/src/external_ts/tsserver_backend.rs";
const MEMBERSHIP_LEDGER: &str = "crates/verter_lsp/src/external_ts/membership_ledger.rs";

/// The ledger advertised/serve-set reader symbols that no PRODUCTION serve path may
/// call until the ledger's read path is wired.
const LEDGER_SERVE_READERS: &[&str] = &[
    "external_files_for_project",
    "advertised_provider_paths_under",
];

/// Whether a PRODUCTION call to `symbol` from `file_rel` is permitted.
fn call_is_allowed(symbol: &str, file_rel: &str) -> bool {
    match symbol {
        // The backend's ledger accessor: NO production caller (its callers are all
        // `#[cfg(test)]` today, skipped by the ancestry tracker before this check).
        "external_files_for_project" => false,
        // The ledger primitive: read ONLY by the `external_files_for_project`
        // accessor's body in tsserver_backend.rs (itself off the serve path) and by
        // its own defining file. Every other production reader is a serve-read.
        "advertised_provider_paths_under" => {
            file_rel == TSSERVER_BACKEND || file_rel == MEMBERSHIP_LEDGER
        }
        _ => true,
    }
}

/// True iff `attrs` carry a `#[cfg(test)]` gate (covering `cfg(test)`,
/// `cfg(all(test, …))`, `cfg(any(test, …))` — the `test` ident anywhere in the cfg
/// predicate). Ident-level (not a string scan), so `cfg(feature = "test-x")` (a
/// STRING literal, not the `test` ident) does NOT match.
fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        match &attr.meta {
            syn::Meta::List(list) => token_stream_has_test_ident(list.tokens.clone()),
            _ => false,
        }
    })
}

/// Recursively scan a token stream for a bare `test` IDENT (descending into nested
/// groups so `all(test, …)` / `any(test, …)` are caught).
fn token_stream_has_test_ident(tokens: proc_macro2::TokenStream) -> bool {
    tokens.into_iter().any(|tt| match tt {
        proc_macro2::TokenTree::Ident(id) => id == "test",
        proc_macro2::TokenTree::Group(g) => token_stream_has_test_ident(g.stream()),
        _ => false,
    })
}

/// AST visitor: records every production (non-`#[cfg(test)]`) call to a ledger
/// serve-set reader outside the allowlist, tracking `#[cfg(test)]` ancestry over
/// `mod` / `fn` / `impl` items.
struct LedgerServeScanner {
    file_rel: String,
    in_cfg_test: bool,
    violations: Vec<String>,
}

impl LedgerServeScanner {
    fn check_call(&mut self, name: &str) {
        if self.in_cfg_test {
            return;
        }
        if LEDGER_SERVE_READERS.contains(&name) && !call_is_allowed(name, &self.file_rel) {
            self.violations.push(format!(
                "{}: PRODUCTION read of ledger serve-set symbol `{name}`. The in-process \
                 MembershipLedger is INTERNAL transition bookkeeping ONLY — live \
                 getExternalFiles is served CROSS-PROCESS from the on-disk store `ready_files` \
                 (the Node plugin reads `carrier_publish_store`), NOT this ledger. No production \
                 serve path may read the ledger's advertised set until the ledger's read \
                 path is wired; route the serve set through the on-disk store.",
                self.file_rel
            ));
        }
    }
}

impl<'ast> Visit<'ast> for LedgerServeScanner {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        let prev = self.in_cfg_test;
        self.in_cfg_test = prev || has_cfg_test(&m.attrs);
        syn::visit::visit_item_mod(self, m);
        self.in_cfg_test = prev;
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let prev = self.in_cfg_test;
        self.in_cfg_test = prev || has_cfg_test(&f.attrs);
        syn::visit::visit_item_fn(self, f);
        self.in_cfg_test = prev;
    }

    fn visit_item_impl(&mut self, im: &'ast syn::ItemImpl) {
        let prev = self.in_cfg_test;
        self.in_cfg_test = prev || has_cfg_test(&im.attrs);
        syn::visit::visit_item_impl(self, im);
        self.in_cfg_test = prev;
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        let prev = self.in_cfg_test;
        self.in_cfg_test = prev || has_cfg_test(&f.attrs);
        syn::visit::visit_impl_item_fn(self, f);
        self.in_cfg_test = prev;
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        let prev = self.in_cfg_test;
        self.in_cfg_test = prev || has_cfg_test(&f.attrs);
        syn::visit::visit_trait_item_fn(self, f);
        self.in_cfg_test = prev;
    }

    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        self.check_call(&mc.method.to_string());
        syn::visit::visit_expr_method_call(self, mc);
    }

    /// Catch the free-fn / associated / fn-pointer path forms too (a method's own
    /// ident is NOT a path segment, so there is no double count with
    /// `visit_expr_method_call`).
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(seg) = path.segments.last() {
            self.check_call(&seg.ident.to_string());
        }
        syn::visit::visit_path(self, path);
    }
}

/// Scan one source string as if it were `file_rel`, returning the violations.
fn scan_source(file_rel: &str, src: &str) -> Vec<String> {
    let Ok(file) = syn::parse_file(src) else {
        return Vec::new();
    };
    let mut scanner = LedgerServeScanner {
        file_rel: file_rel.to_string(),
        in_cfg_test: false,
        violations: Vec::new(),
    };
    scanner.visit_file(&file);
    scanner.violations
}

/// Recursively collect every `.rs` file under `path` whose name does NOT end with
/// `_tests.rs` (an extracted unit-test module file is wholly test-only; the inline
/// `#[cfg(test)]` callers in production files are handled by the ancestry tracker).
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
fn ledger_is_off_the_serve_path() {
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
        "the in-process MembershipLedger must stay OFF the production serve/getExternalFiles \
         path until the ledger's read path is wired:\n  - {}",
        violations.join("\n  - ")
    );
}

/// DISCRIMINATING self-test: prove the scanner FIRES on a production serve-read of
/// each policed symbol, SKIPS the `#[cfg(test)]` callers (inline mod + accessor fn),
/// and ACCEPTS the allowlisted `external_files_for_project` accessor body.
#[test]
fn serve_path_self_test_discriminates() {
    // 1. A PRODUCTION caller of `external_files_for_project` (the premature
    //    serve-read) FIRES — no production caller is permitted.
    let production_serve = scan_source(
        "crates/verter_lsp/src/server/sync_orchestration.rs",
        "fn get_external_files(&self, p: &str) -> Vec<String> {\n\
         self.coordinator.backend().external_files_for_project(p)\n\
         }",
    );
    assert!(
        production_serve
            .iter()
            .any(|v| v.contains("external_files_for_project")),
        "a production caller of external_files_for_project must fire; got {production_serve:?}"
    );

    // 2. The REAL `#[cfg(test)]` callers are SKIPPED: the inline `#[cfg(test)] mod
    //    tests` (workspace_scanner) and the `#[cfg(test)] fn` accessor
    //    (sync_orchestration) — both gate the call under `#[cfg(test)]`.
    let cfg_test_mod = scan_source(
        "crates/verter_lsp/src/workspace_scanner.rs",
        "#[cfg(test)]\n\
         mod tests {\n\
         fn t(backend: &B, p: &str) { let _ = backend.external_files_for_project(p); }\n\
         }",
    );
    assert!(
        cfg_test_mod.is_empty(),
        "a call inside `#[cfg(test)] mod tests` must be skipped; got {cfg_test_mod:?}"
    );
    let cfg_test_fn = scan_source(
        "crates/verter_lsp/src/server/sync_orchestration.rs",
        "impl S {\n\
         #[cfg(test)]\n\
         fn external_ts_advertised_for_project(&self, p: &str) -> Vec<String> {\n\
         self.coordinator.backend().external_files_for_project(p)\n\
         }\n\
         }",
    );
    assert!(
        cfg_test_fn.is_empty(),
        "a call inside a `#[cfg(test)]` fn must be skipped; got {cfg_test_fn:?}"
    );

    // 3. A PRODUCTION reader of the ledger primitive `advertised_provider_paths_under`
    //    from a NON-allowlisted file (a direct serve-read bypassing the accessor)
    //    FIRES.
    let direct_ledger_read = scan_source(
        "crates/verter_lsp/src/server/sync_orchestration.rs",
        "fn serve(&self, p: &ProjectUri) -> Vec<Arc<str>> {\n\
         self.ledger.advertised_provider_paths_under(p)\n\
         }",
    );
    assert!(
        direct_ledger_read
            .iter()
            .any(|v| v.contains("advertised_provider_paths_under")),
        "a production direct read of advertised_provider_paths_under must fire; got \
         {direct_ledger_read:?}"
    );

    // 4. The allowlisted accessor body in tsserver_backend.rs (the
    //    `external_files_for_project` impl reading the ledger primitive) is CLEAN — it
    //    is itself off the serve path because rule 1 forbids any production caller.
    let accessor_ok = scan_source(
        TSSERVER_BACKEND,
        "impl B {\n\
         pub fn external_files_for_project(&self, project: &str) -> Vec<String> {\n\
         self.membership_ledger.advertised_provider_paths_under(&ProjectUri::from(project))\n\
         .into_iter().map(|p| p.to_string()).collect()\n\
         }\n\
         }",
    );
    assert!(
        accessor_ok.is_empty(),
        "the allowlisted accessor body (advertised_provider_paths_under in tsserver_backend.rs) \
         must stay clean; got {accessor_ok:?}"
    );

    // 5. NEGATIVE: an UNRELATED method that merely shares a prefix is NOT policed
    //    (exact-ident discrimination), and the definition file membership_ledger.rs
    //    may reference the primitive internally.
    let unrelated = scan_source(
        "crates/verter_lsp/src/workspace_scanner.rs",
        "fn ok(&self, p: &str) { let _ = self.external_files_for_project_count(p); }",
    );
    assert!(
        unrelated.is_empty(),
        "a prefix-sharing unrelated method (external_files_for_project_count) must NOT fire; \
         got {unrelated:?}"
    );
}
