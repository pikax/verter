//! Slice 3.B focused regression: the
//! `audit_no_hot_loop_instrumentation` architecture guard MUST run
//! cleanly. The same denylist used by the canonical guard in
//! `tests/architecture_guards.rs` is consumed here via `#[path]`
//! mod-include — the constant lives in
//! `tests/audit_hot_loop_denylist.rs`.
//!
//! Discrimination contract:
//! - **Pre-change tree**: no denylist exists, no guard runs, this
//!   file does not compile (missing test helper) AND any compile
//!   producer emit landing inside one of the listed hot loops would
//!   not be observable until profiling caught the regression.
//! - **Post-change tree**: the AST visitor finds every denylisted
//!   function body and confirms each is free of audit-emit calls.
//!
//! This file replicates the same scan as the canonical guard so that
//! a regression is caught even when the umbrella `architecture_guards`
//! integration test binary is filtered out of a CI run by name.

#[path = "../audit_hot_loop_denylist.rs"]
mod audit_hot_loop_denylist;

use std::collections::HashMap;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn walk_dir_collect_rs(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path)) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir_collect_rs(&path, f);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            f(&path);
        }
    }
}

/// Compute the module-path stack for a file relative to its
/// crate's `src/` root.
fn module_stack_for_file(crate_src: &std::path::Path, file: &std::path::Path) -> Vec<String> {
    let rel = match file.strip_prefix(crate_src) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut segments: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(last) = segments.last_mut() {
        if let Some(stripped) = last.strip_suffix(".rs") {
            *last = stripped.to_string();
        }
    }
    match segments.last().map(|s| s.as_str()) {
        Some("lib") | Some("main") => {
            if segments.len() == 1 {
                return Vec::new();
            }
            segments.pop();
        }
        Some("mod") => {
            segments.pop();
        }
        _ => {}
    }
    segments
}

#[test]
fn slice_3b_audit_no_hot_loop_instrumentation_focused_regression() {
    use syn::visit::Visit;

    let denylist = audit_hot_loop_denylist::HOT_PATH_DENYLIST;
    assert!(!denylist.is_empty(), "denylist must not be empty",);

    // Match only `current_observer` — the canonical TLS accessor
    // producers must use to reach the audit substrate. Session-side
    // helpers that share method names with `AuditObserver` (e.g.
    // `RequestContextLike::record_cache_event`) are NOT producer
    // emits and must not be flagged.
    const AUDIT_EMIT_FUNCTION_NAMES: &[&str] = &["current_observer"];

    struct EmitFinder {
        violations: Vec<String>,
    }
    impl<'ast> Visit<'ast> for EmitFinder {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(p) = &*call.func {
                if let Some(last) = p.path.segments.last() {
                    let name = last.ident.to_string();
                    if AUDIT_EMIT_FUNCTION_NAMES.contains(&name.as_str()) {
                        self.violations.push(name);
                    }
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
    }

    struct Walker<'a> {
        target_paths: &'a HashMap<&'a str, Vec<(usize, &'a str)>>,
        path_stack: Vec<String>,
        violations: Vec<(usize, String)>,
        matched: Vec<bool>,
        current_crate: &'a str,
    }
    impl<'a, 'ast> Visit<'ast> for Walker<'a> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            self.path_stack.push(item.ident.to_string());
            syn::visit::visit_item_mod(self, item);
            self.path_stack.pop();
        }
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            let segment = if let syn::Type::Path(tp) = &*item.self_ty {
                tp.path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_else(|| "<impl>".into())
            } else {
                "<impl>".into()
            };
            self.path_stack.push(segment);
            syn::visit::visit_item_impl(self, item);
            self.path_stack.pop();
        }
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            self.path_stack.push(item.sig.ident.to_string());
            self.check(&item.block);
            syn::visit::visit_item_fn(self, item);
            self.path_stack.pop();
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            self.path_stack.push(item.sig.ident.to_string());
            self.check(&item.block);
            syn::visit::visit_impl_item_fn(self, item);
            self.path_stack.pop();
        }
    }
    impl<'a> Walker<'a> {
        fn check(&mut self, block: &syn::Block) {
            let path = self.path_stack.join("::");
            let Some(targets) = self.target_paths.get(self.current_crate) else {
                return;
            };
            for (idx, target_path) in targets {
                if &path == target_path {
                    self.matched[*idx] = true;
                    let mut finder = EmitFinder {
                        violations: Vec::new(),
                    };
                    finder.visit_block(block);
                    for name in finder.violations {
                        self.violations.push((*idx, name));
                    }
                }
            }
        }
    }

    let mut by_crate: HashMap<&str, Vec<(usize, &str)>> = HashMap::new();
    for (idx, (krate, path)) in denylist.iter().enumerate() {
        by_crate.entry(krate).or_default().push((idx, path));
    }

    let mut matched = vec![false; denylist.len()];
    let mut violations: Vec<String> = Vec::new();

    for krate in by_crate.keys() {
        let crate_src = workspace_root().join("crates").join(krate).join("src");
        if !crate_src.exists() {
            panic!(
                "Slice 3.B focused regression: crate `{krate}` listed in denylist \
                 but `crates/{krate}/src/` does not exist; denylist is stale."
            );
        }
        // Leaf identifier names of THIS crate's denylisted function paths. A
        // file can host a denylisted function (the staleness `matched` signal)
        // only if it contains that leaf name, and a VIOLATION only if it also
        // contains `current_observer`. Either is necessary — a file with
        // neither cannot affect the result, so skip its parse.
        let crate_leaf_names: Vec<&str> = by_crate[krate]
            .iter()
            .map(|(_, p)| p.rsplit("::").next().unwrap_or(p))
            .collect();
        walk_dir_collect_rs(&crate_src, &mut |path: &std::path::Path| {
            let src = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => return,
            };
            // Textual pre-filter (coverage-identical).
            if !src.contains("current_observer")
                && !crate_leaf_names.iter().any(|n| src.contains(n))
            {
                return;
            }
            let parsed = match syn::parse_file(&src) {
                Ok(p) => p,
                Err(_) => return,
            };
            let initial_stack = module_stack_for_file(&crate_src, path);
            let mut walker = Walker {
                target_paths: &by_crate,
                path_stack: initial_stack,
                violations: Vec::new(),
                matched: vec![false; denylist.len()],
                current_crate: krate,
            };
            walker.visit_file(&parsed);
            for (idx, name) in walker.violations {
                violations.push(format!(
                    "  - [{}] {} :: {} — emit `{}`",
                    krate,
                    by_crate[krate]
                        .iter()
                        .find(|(i, _)| *i == idx)
                        .map(|(_, p)| *p)
                        .unwrap_or("<unknown>"),
                    path.display(),
                    name,
                ));
            }
            for (i, m) in walker.matched.iter().enumerate() {
                if *m {
                    matched[i] = true;
                }
            }
        });
    }

    let stale: Vec<String> = matched
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            if !m {
                let (krate, path) = denylist[i];
                Some(format!("  - {krate} :: {path}"))
            } else {
                None
            }
        })
        .collect();

    assert!(
        stale.is_empty(),
        "Slice 3.B focused regression: denylist entries did NOT match any \
         function in the corresponding source tree. Denylist is stale:\n{}",
        stale.join("\n"),
    );

    assert!(
        violations.is_empty(),
        "Slice 3.B focused regression: producer-side audit emits MUST NOT \
         appear inside the hot-path denylist. Move emits to phase \
         boundaries. Found:\n{}",
        violations.join("\n"),
    );
}
