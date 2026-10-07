use super::*;

/// Tier 1A guard 3 — `OwnedEvalProgram` MUST be `Send + Sync +
/// 'static`. Compile-time `assert_impl_all!` guards in the production
/// source file enforce this; the test here makes the assertion
/// observable in the `cargo test` output and asserts the structural
/// invariant via syn-AST inspection (no lifetime parameter, no
/// `Rc`/`Cell`-typed field).
#[test]
fn no_owned_artifact_holds_borrowed_lifetime() {
    // Side 1: `assert_impl_all!`-style runtime guard. If a regression
    // re-introduced `Rc<...>`, the type would lose Send and the bound
    // would fail to compile (this test file would fail to build).
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<verter_session::owned_artifacts::eval_program::OwnedEvalProgram>();

    // Side 2: structural invariant. Walk the syn-AST of the source
    // file and verify the canonical struct carries NO lifetime
    // parameter. This catches future regressions where someone adds a
    // `pub struct OwnedEvalProgram<'a>` form (which would be a step
    // back toward the borrowed-form contract).
    for (path, struct_name) in [(
        "crates/verter_session/src/owned_artifacts/eval_program.rs",
        "OwnedEvalProgram",
    )] {
        let body = read_workspace_file(path);
        let parsed: syn::File = syn::parse_str(&body).expect("parse owned-artifact module");
        let mut found = false;
        for item in &parsed.items {
            if let syn::Item::Struct(s) = item {
                if s.ident == struct_name {
                    found = true;
                    let has_lifetime = s.generics.lifetimes().next().is_some();
                    assert!(
                        !has_lifetime,
                        "Tier 1A guard `no_owned_artifact_holds_borrowed_lifetime`: \
                         `{struct_name}` MUST carry no lifetime parameter; \
                         a regression in {path} re-introduced one."
                    );
                }
            }
        }
        assert!(
            found,
            "{struct_name} not found in {path} (test self-broken)"
        );
    }
}

#[test]
fn audit_counter_single_helper() {
    let src = read_workspace_file("crates/verter_type_engine/src/semantic_query_memo/mod.rs");
    let violations = audit_counter_helper_violations(&src);

    assert!(
        violations.is_empty(),
        "audit_counter_single_helper: every bump of \
         `inflight_aborted_retries` or `cold_aborts_swept` must go \
         through `record_inflight_aborted_retry` / \
         `record_cold_abort_swept` (see CLAUDE.md Decision #5). \
         Direct `self.stats.<counter>.fetch_add` outside the helper \
         bodies is forbidden because it lets the global aggregate \
         and the per-request mirror diverge silently. Found:\n  {}",
        violations.join("\n  ")
    );
}

/// Self-test for `audit_counter_single_helper`. Drives the SAME
/// production matcher (`audit_counter_helper_violations`) against a
/// synthetic source string so a regression in the matcher's helper
/// detection or 64-byte lookahead would surface here as well — not
/// just on the live tree.
#[test]
fn audit_counter_single_helper_discriminator_self_test() {
    // Synthetic violator: bump outside any helper body.
    let synthetic_violator = "\
fn unrelated_function() {
    self.stats.inflight_aborted_retries.fetch_add(1, Ordering::Relaxed);
}
";
    let v = audit_counter_helper_violations(synthetic_violator);
    assert_eq!(
        v.len(),
        1,
        "audit_counter_single_helper matcher must catch a synthetic \
         violation outside any helper body — got {} violations: {v:?}",
        v.len(),
    );

    // Synthetic clean: same bump INSIDE a helper body must NOT flag.
    let synthetic_helper = "\
fn record_inflight_aborted_retry(stats: &AtomicSemanticGraphStats) {
    stats.inflight_aborted_retries.fetch_add(1, Ordering::Relaxed);
}
";
    let v = audit_counter_helper_violations(synthetic_helper);
    assert_eq!(
        v.len(),
        0,
        "audit_counter_single_helper matcher must NOT flag fetch_add \
         inside a helper body — got {} false positives: {v:?}",
        v.len(),
    );

    // Synthetic multi-line method-chain violator: confirms the
    // 64-byte lookahead window catches `\n    .fetch_add` splits.
    // A regression that shrank the lookahead to e.g. 16 bytes would
    // miss this and silently weaken the live guard.
    let synthetic_multiline = "\
fn unrelated_function() {
    self.stats
        .inflight_aborted_retries
        .fetch_add(1, Ordering::Relaxed);
}
";
    let v = audit_counter_helper_violations(synthetic_multiline);
    assert_eq!(
        v.len(),
        1,
        "audit_counter_single_helper matcher must catch a multi-line \
         method-chain violation — the 64-byte lookahead window covers \
         this case. Got {} violations: {v:?}",
        v.len(),
    );
}

/// `HostAuditRuntime::active_requests` is private; the lifecycle
/// methods that mutate it must each have exactly ONE in-tree call
/// site inside `host_audit_runtime.rs`.
#[test]
fn audit_request_registration_lifecycle() {
    use std::path::PathBuf;
    use syn::visit::Visit;
    let root = workspace_root();

    let allowed_file = "crates/verter_session/src/host_audit_runtime.rs";
    let methods: &[&str] = &[
        "register_active_request",
        "finalize_active_request",
        "drop_active_request",
    ];

    /// Visitor that collects every method-call expression matching
    /// the lifecycle vocabulary.
    struct LifecycleCallCollector<'m> {
        methods: &'m [&'m str],
        hits: Vec<String>,
    }

    impl<'ast, 'm> Visit<'ast> for LifecycleCallCollector<'m> {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            let name = call.method.to_string();
            if self.methods.contains(&name.as_str()) {
                self.hits.push(name);
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut violations: Vec<String> = Vec::new();
    let crates_dir: PathBuf = root.join("crates");

    walk_dir_collect_rs(&crates_dir, &mut |path: &std::path::Path| {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == allowed_file {
            return;
        }
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("audit_request_registration_lifecycle: cannot read `{rel}`: {e}")
        });
        // Textual pre-filter (coverage-identical): a forbidden call
        // `x.register_active_request(...)` MUST contain that method name
        // as a substring, so a file containing none of the three lifecycle
        // names cannot possibly host a violation — skip the expensive parse.
        if !methods.iter().any(|m| src.contains(m)) {
            return;
        }
        let parsed = match syn::parse_file(&src) {
            Ok(p) => p,
            // Files we cannot parse (e.g. macro-generated bodies)
            // are skipped — `syn::parse_file` rejects very few real
            // crate files. The earlier substring scan acted as the
            // safety net here; we keep the hard panic for unparseable
            // files outside of `tests/` because that would indicate a
            // code-corruption signal worth surfacing.
            Err(e) => {
                if rel.starts_with("crates/") && rel.contains("/src/") && !rel.contains("/tests/") {
                    panic!("audit_request_registration_lifecycle: cannot parse `{rel}`: {e}");
                }
                return;
            }
        };
        let mut collector = LifecycleCallCollector {
            methods,
            hits: Vec::new(),
        };
        collector.visit_file(&parsed);
        for hit in collector.hits {
            violations.push(format!("{rel}: {hit}"));
        }
    });

    assert!(
        violations.is_empty(),
        "audit_request_registration_lifecycle: the three lifecycle methods on \
         `HostAuditRuntime` must each have exactly ONE in-tree call site, all \
         inside `crates/verter_session/src/host_audit_runtime.rs`. Found callers \
         outside that file:\n  {}",
        violations.join("\n  ")
    );
}

/// Slice 3.B architecture guard — instrumentation lives at phase
/// boundaries only, never inside hot loops.
///
/// Reads the canonical `(crate, function_path)` denylist from
/// `audit_hot_loop_denylist::HOT_PATH_DENYLIST` and rejects any
/// `current_observer()` call (the audit substrate's TLS accessor)
/// inside the body of any listed function. Phase boundaries (parse /
/// transform / codegen / css_analysis / sourcemap) fire O(1) times
/// per request — that is the only permitted granularity.
#[test]
fn audit_no_hot_loop_instrumentation() {
    use std::collections::HashMap;
    use syn::visit::Visit;

    let denylist = audit_hot_loop_denylist::HOT_PATH_DENYLIST;
    assert!(
        !denylist.is_empty(),
        "audit_no_hot_loop_instrumentation: denylist must not be empty — \
         the guard is meaningless without at least one hot-path entry. \
         If the system genuinely has no hot loops worth listing, escalate \
         this guard's purpose for review.",
    );
    assert!(
        denylist.len() <= 20,
        "audit_no_hot_loop_instrumentation: denylist has {} entries (> 20); \
         the design guidance is 4–8 typical / ~20 max. Escalate before \
         adding more — broad lists usually indicate misplaced \
         instrumentation rather than additional hot loops.",
        denylist.len(),
    );

    // The audit-substrate's TLS observer accessor — the canonical
    // entry point producers use to reach `AuditObserver::record_*`.
    // Without `verter_audit::current_observer()` (or the unqualified
    // `current_observer()` form), no producer-side audit emit is
    // possible. Matching just this name keeps the guard precise:
    // session-internal helpers that share verbs with the substrate
    // trait (e.g. `RequestContextLike::record_cache_event` on the
    // scheduler-side request-context handle) are NOT producer audit
    // emits and must not be flagged. The cross-crate audit-substrate
    // surface is identified by the `current_observer` accessor; that
    // is the gate the guard enforces.
    const AUDIT_EMIT_FUNCTION_NAMES: &[&str] = &["current_observer"];

    /// Inner visitor — scans a single function body for audit-emit
    /// call sites without descending into nested `fn`/`impl` items.
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

    /// Outer visitor — assembles fully-qualified function paths and
    /// runs `EmitFinder` against any body whose path appears in the
    /// denylist. The path stack is seeded with the file's own
    /// module path (derived from `<crate_src>/<sub>/<sub>/foo.rs` →
    /// `sub::sub::foo`); inline `mod xxx { ... }` blocks and
    /// `impl Type` blocks then push further segments.
    struct DenyVisitor<'a> {
        target_paths: &'a HashMap<&'a str, Vec<(usize, &'a str)>>,
        path_stack: Vec<String>,
        violations: Vec<(usize, String)>,
        matched: Vec<bool>,
        current_crate: &'a str,
    }
    impl<'a, 'ast> Visit<'ast> for DenyVisitor<'a> {
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
    impl<'a> DenyVisitor<'a> {
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

    let mut by_crate: HashMap<&str, Vec<(usize, &str)>> = HashMap::new();
    for (idx, (krate, path)) in denylist.iter().enumerate() {
        by_crate.entry(krate).or_default().push((idx, path));
    }

    let mut matched: Vec<bool> = vec![false; denylist.len()];
    let mut all_violations: Vec<String> = Vec::new();

    for krate in by_crate.keys() {
        let crate_src = workspace_root().join("crates").join(krate).join("src");
        if !crate_src.exists() {
            panic!(
                "audit_no_hot_loop_instrumentation: crate `{krate}` listed in denylist \
                 but `crates/{krate}/src/` does not exist; the denylist is stale."
            );
        }
        // Leaf identifier names of THIS crate's denylisted function paths
        // (`mod::sub::fn_name` → `fn_name`). A file can only HOST a denylisted
        // function (the `matched`/staleness signal) if it contains that leaf
        // name in its text; it can only host a VIOLATION if it also contains
        // `current_observer`. Either condition is necessary, so a file
        // containing none of them cannot affect the result — skip the parse.
        let crate_leaf_names: Vec<&str> = by_crate[krate]
            .iter()
            .map(|(_, p)| p.rsplit("::").next().unwrap_or(p))
            .collect();
        walk_dir_collect_rs(&crate_src, &mut |path: &std::path::Path| {
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                panic!(
                    "audit_no_hot_loop_instrumentation: cannot read `{}`: {e}",
                    path.display()
                )
            });
            // Textual pre-filter (coverage-identical): keep the file only if it
            // can contribute to either result dimension.
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
            let mut visitor = DenyVisitor {
                target_paths: &by_crate,
                path_stack: initial_stack,
                violations: Vec::new(),
                matched: vec![false; denylist.len()],
                current_crate: krate,
            };
            visitor.visit_file(&parsed);
            for (i, m) in visitor.matched.iter().enumerate() {
                if *m {
                    matched[i] = true;
                }
            }
            for (idx, name) in visitor.violations {
                all_violations.push(format!(
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
        "audit_no_hot_loop_instrumentation: the following denylist entries did NOT \
         match any function in the corresponding crate's source tree. The denylist \
         is stale — the function was renamed, moved, or removed. Update the \
         denylist in `tests/cases/support/audit_hot_loop_denylist.rs`:\n{}",
        stale.join("\n"),
    );

    assert!(
        all_violations.is_empty(),
        "audit_no_hot_loop_instrumentation: producer-side audit emits are FORBIDDEN \
         inside the hot-path denylist. Move the emit to a phase boundary (parse / \
         transform / codegen / css_analysis / sourcemap) outside the inner loop. \
         Found:\n{}",
        all_violations.join("\n"),
    );
}

/// Self-test for `audit_no_hot_loop_instrumentation` discrimination.
/// Confirms the matcher detects `current_observer()` calls and does
/// NOT misclassify session-internal helpers (e.g. `record_cache_event`
/// on `RequestContextLike`) as substrate emits.
#[test]
fn audit_no_hot_loop_instrumentation_self_test_rejects_emit_names() {
    use syn::visit::Visit;

    let synthetic_violator = "\
        fn busy_loop() {\n\
        \x20\x20    let _ = verter_audit::current_observer();\n\
        \x20\x20    drop(verter_audit::current_observer());\n\
        }\n\
    ";
    let synthetic_clean = "\
        fn busy_loop() {\n\
        \x20\x20    let _ = self.scratch.len();\n\
        \x20\x20    let _ = self.allocator.alloc_str(\"hi\");\n\
        \x20\x20    let _ = ctx.0.record_cache_event(CacheEventKind::Hit);\n\
        }\n\
    ";

    fn count_emits(src: &str) -> usize {
        let parsed = syn::parse_file(src).expect("parse synthetic");
        struct Counter(usize);
        impl<'ast> Visit<'ast> for Counter {
            fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
                if let syn::Expr::Path(p) = &*call.func {
                    if let Some(last) = p.path.segments.last() {
                        if last.ident == "current_observer" {
                            self.0 += 1;
                        }
                    }
                }
                syn::visit::visit_expr_call(self, call);
            }
        }
        let mut c = Counter(0);
        c.visit_file(&parsed);
        c.0
    }

    assert_eq!(
        count_emits(synthetic_violator),
        2,
        "self-test: synthetic violator with two `current_observer()` calls \
         MUST produce exactly 2 detected emits. A regression that drops \
         `current_observer` from the matcher would fail this assertion \
         before the live guard can silently weaken.",
    );
    assert_eq!(
        count_emits(synthetic_clean),
        0,
        "self-test: synthetic clean body MUST produce zero detected emits — \
         note the body deliberately includes a session-internal \
         `ctx.0.record_cache_event(...)` to confirm the matcher does NOT \
         flag session-side helpers that share verbs with the substrate's \
         `AuditObserver` trait.",
    );
}

/// Wave 3 close — `audit_observer_single_accessor`.
///
/// Lower crates must reach the audit substrate exclusively through
/// [`verter_audit::current_observer`]. They must NOT reach into
/// `verter_type_engine::request_context::current_request_context` (which
/// is a session-internal, typed accessor onto the concrete
/// `Arc<RequestContext>`). Architectural intent: the substrate's
/// thin `AuditObserver` trait is the cross-crate API; only the
/// session crate (which owns `RequestContext`) is permitted to use
/// the typed accessor.
///
/// `verter_session/` and `verter_audit/` are explicitly out of
/// scope: `verter_session` defines and consumes
/// `current_request_context`, and `verter_audit` is the substrate.
/// `verter_scheduler/` is also out of scope: it documents
/// `current_request_context` in module-level comments but the
/// scheduler does not call it (the scheduler crate's own TLS
/// accessor is `verter_execution::request_context::current_request_id`).
///
/// In-scope crates (the 5 lower-crate consumers of audit):
///
///   - `verter_compiler`
///   - `verter_semantic`
///   - `verter_workspace`
///   - `verter_lsp`
///   - `verter_mcp_server`
///
/// Discrimination contract:
/// - Pre-change tree (5 lower crates currently clean): the guard
///   passes; no in-scope file references `current_request_context`.
/// - Regression: any new code in the 5 in-scope crates that adds a
///   `current_request_context` call appears in `violations` and
///   fails the assertion. The fix is to migrate the call site to
///   `verter_audit::current_observer()` (which yields an
///   `Arc<dyn AuditObserver>`) and emit through the trait.
/// - Allow-list: legitimate pre-existing call sites are listed in
///   `ALLOW_LIST` with the rationale. Empty today.
#[test]
fn audit_observer_single_accessor() {
    // Allow-list: `(crate, relative_path_within_crate, rationale)`
    // tuples for pre-existing legitimate call sites that predate
    // `verter_audit::current_observer()` and have not yet migrated.
    // Empty today — the 5 lower crates are all clean.
    const ALLOW_LIST: &[(&str, &str, &str)] = &[];

    // The 5 in-scope lower crates. Adding a new lower crate that
    // emits audit events requires extending this list, but the
    // architectural rule remains: lower crates use
    // `verter_audit::current_observer()`.
    const IN_SCOPE_CRATES: &[&str] = &[
        "verter_compiler",
        "verter_semantic",
        "verter_workspace",
        "verter_lsp",
        "verter_mcp_server",
    ];

    // The forbidden pattern. Substring match (the canonical form is
    // `current_request_context()` — both fully qualified and
    // bare-imported usages contain this substring).
    const FORBIDDEN: &str = "current_request_context";

    let mut violations: Vec<String> = Vec::new();
    let mut allow_list_hits: Vec<(usize, String)> = Vec::new();

    for krate in IN_SCOPE_CRATES {
        let crate_src = workspace_root().join("crates").join(krate).join("src");
        if !crate_src.exists() {
            panic!(
                "audit_observer_single_accessor: crate `{krate}` listed as in-scope \
                 but `crates/{krate}/src/` does not exist; the in-scope list is stale."
            );
        }
        walk_dir_collect_rs(&crate_src, &mut |path: &std::path::Path| {
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                panic!(
                    "audit_observer_single_accessor: cannot read `{}`: {e}",
                    path.display()
                )
            });
            // Compute path relative to the crate's src/ for stable
            // allow-list keys.
            let rel = path.strip_prefix(&crate_src).unwrap_or(path);
            let rel_str = rel.to_string_lossy().replace('\\', "/").to_string();
            for (line_no, line) in src.lines().enumerate() {
                // Skip line/block comments — comment-side mentions
                // (e.g. doc strings cross-referencing the session
                // accessor) are NOT call-site bypasses.
                let trimmed = line.trim_start();
                if trimmed.starts_with("//")
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("//!")
                {
                    continue;
                }
                if !line.contains(FORBIDDEN) {
                    continue;
                }
                // Walk the allow-list for this exact (crate, rel_path).
                let allow_idx = ALLOW_LIST
                    .iter()
                    .position(|(k, p, _r)| *k == *krate && *p == rel_str.as_str());
                let entry = format!("  - [{krate}] {rel_str}:{}: {}", line_no + 1, line.trim());
                match allow_idx {
                    Some(i) => allow_list_hits.push((i, entry)),
                    None => violations.push(entry),
                }
            }
        });
    }

    // Stale allow-list detection: every allow-list entry must have
    // matched at least one line. Otherwise the entry is stale (the
    // call site was deleted or migrated) and should be removed.
    let stale: Vec<String> = ALLOW_LIST
        .iter()
        .enumerate()
        .filter_map(|(i, (k, p, r))| {
            if allow_list_hits.iter().all(|(idx, _)| *idx != i) {
                Some(format!("  - [{k}] {p} (rationale: {r})"))
            } else {
                None
            }
        })
        .collect();

    assert!(
        stale.is_empty(),
        "audit_observer_single_accessor: the following allow-list entries did NOT \
         match any line in their crate's source tree. Either the call site was \
         deleted/migrated (drop the entry) or the path/crate is wrong (fix the entry):\n{}",
        stale.join("\n"),
    );

    assert!(
        violations.is_empty(),
        "audit_observer_single_accessor: lower crates must reach the audit substrate \
         through `verter_audit::current_observer()` only. The following call sites \
         use `current_request_context` (the session-internal typed accessor) instead. \
         Migrate to `current_observer()` and emit through the `AuditObserver` trait, \
         OR — if the call site genuinely needs the typed `Arc<RequestContext>` for a \
         legitimate reason that predates the substrate split — add an allow-list entry \
         in this guard with the rationale.\n\nViolations:\n{}",
        violations.join("\n"),
    );
}

/// Wave 3 close — `wave_3_entry_points_propagate_tls`.
///
/// Every Wave-3-added `*_with_audit` entry-point must have a
/// corresponding TLS-propagation test that drives it via the
/// [`verter_session::tests::audit_tls_harness::assert_observer_reaches`]
/// harness (or — for entry-points where a stricter custom assertion
/// is more discriminating — drives the entry-point and asserts the
/// observer reaches the producer crate's instrumentation).
///
/// The guard is parameterised by `WAVE_3_ENTRY_POINTS` — a list of
/// `(entry_point_symbol, paired_test_files)` tuples. For each
/// entry-point, the guard verifies that at least one of the listed
/// test files contains BOTH:
///
///   1. an invocation of `entry_point_symbol` (the function/method
///      name appears in the file), AND
///   2. an `assert_observer_reaches(...)` call (the §4 Wave 1.5
///      harness's primary verification primitive).
///
/// A `MISSING_TLS_TEST` allow-list documents Wave-3 entry-points
/// that ship without a paired TLS-propagation test. Each entry
/// carries a rationale and is intended as a temporary marker — the
/// follow-up fix-pass adds the missing test.
///
/// Discrimination contract:
/// - Pre-change tree (Wave 3 not yet integrated): the entry-point
///   methods do not exist, so the test files cannot reference them
///   either; the guard fails with "entry-point method missing".
/// - Wave-3 entry-point landed without a paired TLS test: appears
///   in the missing-pair list (and must either get a test added or
///   an allow-list entry).
/// - Wave-3 entry-point with a discriminating TLS test: passes.
#[test]
fn wave_3_entry_points_propagate_tls() {
    // `(entry_point_symbol, &[paired_test_file_relative_paths])`
    //
    // `entry_point_symbol`: the function/method name producers/tests
    // invoke. The guard substring-matches this in the paired test
    // files.
    //
    // `paired_test_file_relative_paths`: relative-to-workspace test
    // file paths. The guard checks that AT LEAST ONE listed file
    // contains both the entry-point symbol and an
    // `assert_observer_reaches` call.
    const WAVE_3_ENTRY_POINTS: &[(&str, &[&str])] = &[
        // Slice 3.A — TypeResolution producer.
        // `resolve_type_with_audit` (verter_session) drives the
        // `RequestKind::TypeResolution` audit producer. The TLS
        // driver asserts the dispatcher's hop accounting increments
        // (proving `current_observer()` was reachable on the
        // dispatch path) and that the harness's outer guard remains
        // visible on the calling thread after the entry-point's
        // nested guard drops.
        (
            "resolve_type_with_audit",
            &["crates/verter_session/tests/cases/g_type/type_resolution_audit_tls_propagation.rs"],
        ),
        // Slice 3.B — Compile producer.
        // `compile_with_audit` (verter_session) drives the
        // `RequestKind::Compile` audit producer. The cross-crate
        // TLS harness exercises this: harness drives
        // `compile_with_audit`, asserts producer-crate
        // (`verter_compiler::code_transform`) instrumentation
        // observed `Some(observer)` via the `code_transform_ops > 0`
        // discriminator.
        (
            "compile_with_audit",
            &["crates/verter_session/tests/cases/g_misc0/tls_harness_cross_crate.rs"],
        ),
        // Slice 3.C — SemanticAnalysis producer.
        // `analyze_with_audit` (verter_session) drives the
        // `RequestKind::SemanticAnalysis` audit producer. The
        // dedicated TLS test asserts the substrate slot is populated
        // for the audit window and drained on return.
        (
            "analyze_with_audit",
            &["crates/verter_session/tests/cases/g_misc0/semantic_analysis_audit_tls_propagation.rs"],
        ),
        // Slice 3.D — Workspace producer.
        // `audit_op` is a trait method on `WorkspaceAccess`; the
        // session-level wrapper `audit_workspace_op` installs the
        // `RequestContextGuard` BEFORE the workspace traversal so
        // the trait body sees `current_observer() == Some(_)`. The
        // TLS driver drives the wrapper through the harness and
        // asserts the trait body reaches the resolver and stamps a
        // non-zero request id (proving the TLS slot was visible).
        // Test placement is verter_session/tests/ rather than
        // verter_workspace/tests/ because the harness lives in
        // verter_session and adding a dev-dep on verter_session
        // from verter_workspace would form a circular test-target
        // cycle; the existing slice
        // `workspace_audit_production_callsite.rs` resolves the
        // same constraint by living here too.
        (
            "audit_op",
            &["crates/verter_session/tests/cases/g_misc0/workspace_audit_tls_propagation.rs"],
        ),
        // Slice 3.E — LSP producer.
        // `run_with_audit` (verter_lsp::audit_harness) wraps every
        // LSP `*_with_audit` handler. The TLS driver wraps a
        // synthetic handler future and asserts the substrate
        // observer is visible inside the future when audit is
        // enabled, and absent when audit is disabled
        // (short-circuit path).
        (
            "run_with_audit",
            &["crates/verter_lsp/tests/cases/lsp_audit_tls_propagation.rs"],
        ),
        // Slice 3.F — Mcp producer.
        // `audit_mcp_tool_call` (verter_session) wraps a synthetic
        // tool-callback closure with the standard registration /
        // RequestContextGuard / finalize lifecycle. The TLS driver
        // wraps a synthetic closure and asserts the substrate
        // observer is visible inside the closure body when audit is
        // enabled, and absent when audit is disabled (Noop arm).
        (
            "audit_mcp_tool_call",
            &["crates/verter_session/tests/cases/g_misc0/mcp_audit_tls_propagation.rs"],
        ),
        // FlowReturnInference producer.
        // `get_flow_return_type_with_audit` (verter_session) drives
        // the `RequestKind::FlowReturnInference` audit producer. The
        // TLS driver asserts the cold flow evaluation's
        // `cold_computes` counter increments (proving the request
        // context installed by the entry-point's guard was reachable
        // at the `FlowReturnStarted` emission site) and that the
        // harness's outer guard remains visible after the
        // entry-point's nested guard drops.
        (
            "get_flow_return_type_with_audit",
            &["crates/verter_session/tests/cases/g_misc0/flow_return_audit_tls_propagation.rs"],
        ),
    ];

    // Wave-3 entry-points that ship WITHOUT a paired TLS test.
    // Each entry: `(entry_point_symbol, rationale)`.
    //
    // Architectural intent: this list shrinks toward zero. The
    // follow-up fix-pass adds the missing TLS-propagation test for
    // each entry below, then removes the allow-list entry.
    //
    // The guard rejects an allow-list entry whose entry-point
    // already has a paired TLS test (stale allow-list).
    const MISSING_TLS_TEST: &[(&str, &str)] = &[];

    let workspace = workspace_root();

    // Step 1: every entry-point with a paired test must have at
    // least one test that references both the entry-point symbol
    // and `assert_observer_reaches`.
    // Resolve a pinned candidate path to a real file. Test files
    // consolidated into group binaries move to a subdirectory
    // (tests/g_<group>/<name>.rs), so a pinned top-level path may be
    // stale; fall back to locating the file by basename under the
    // crate's tests/ tree. The guard validates CONTENT (the entry-point
    // symbol plus `assert_observer_reaches`), not the exact path, so a
    // relocated file remains a valid pin.
    let resolve_pinned = |rel: &str| -> Option<std::path::PathBuf> {
        let direct = workspace.join(rel);
        if direct.exists() {
            return Some(direct);
        }
        let p = std::path::Path::new(rel);
        let base = p.file_name()?;
        let mut comps = p.components();
        let c0 = comps.next()?; // crates
        let c1 = comps.next()?; // <crate>
        let c2 = comps.next()?; // tests
        let tests_root = workspace
            .join(c0.as_os_str())
            .join(c1.as_os_str())
            .join(c2.as_os_str());
        walkdir::WalkDir::new(&tests_root)
            .into_iter()
            .flatten()
            .find(|e| e.file_name() == base)
            .map(|e| e.path().to_path_buf())
    };

    let mut missing: Vec<String> = Vec::new();
    let mut wrong_path: Vec<String> = Vec::new();
    for (symbol, candidate_files) in WAVE_3_ENTRY_POINTS {
        let mut any_match = false;
        for rel in *candidate_files {
            let abs = match resolve_pinned(rel) {
                Some(a) => a,
                None => {
                    wrong_path.push(format!("  - {symbol} → {rel} (file does not exist)"));
                    continue;
                }
            };
            let src = std::fs::read_to_string(&abs).unwrap_or_else(|e| {
                panic!("wave_3_entry_points_propagate_tls: cannot read `{rel}`: {e}")
            });
            let drives_entry = src.contains(symbol);
            let uses_harness = src.contains("assert_observer_reaches");
            if drives_entry && uses_harness {
                any_match = true;
                break;
            }
        }
        if !any_match {
            missing.push(format!(
                "  - {symbol}: none of {:?} contains BOTH the entry-point symbol AND \
                 `assert_observer_reaches(...)`",
                candidate_files,
            ));
        }
    }

    assert!(
        wrong_path.is_empty(),
        "wave_3_entry_points_propagate_tls: paired test files reference paths that \
         do not exist. The guard's pin list is stale. Update WAVE_3_ENTRY_POINTS in \
         this test:\n{}",
        wrong_path.join("\n"),
    );

    assert!(
        missing.is_empty(),
        "wave_3_entry_points_propagate_tls: the following Wave-3 entry-points are \
         pinned to test files that do NOT both invoke the entry-point AND drive \
         `assert_observer_reaches(...)`. Either add a TLS-propagation test for the \
         entry-point, or — if the entry-point cannot yet be tested via the harness \
         — move the entry to MISSING_TLS_TEST with a rationale.\n{}",
        missing.join("\n"),
    );

    // Step 2: every entry in MISSING_TLS_TEST is a temporary
    // allow-list. If a paired TLS-propagation test now exists, the
    // entry must be promoted to WAVE_3_ENTRY_POINTS (and removed
    // from this list). Detect that by scanning all
    // `crates/*/tests/*tls*propagation*.rs` and
    // `crates/*/tests/*tls_harness*.rs` plus any test file that
    // already uses `assert_observer_reaches` for a co-occurrence
    // with the entry-point symbol.
    let mut tls_test_files: Vec<std::path::PathBuf> = Vec::new();
    let crates_dir = workspace.join("crates");
    let crate_entries = std::fs::read_dir(&crates_dir)
        .unwrap_or_else(|e| panic!("wave_3_entry_points_propagate_tls: cannot read crates/: {e}"));
    // Skip the guard file itself — it lists every entry-point
    // symbol (in WAVE_3_ENTRY_POINTS / MISSING_TLS_TEST) AND the
    // string `assert_observer_reaches` (in this guard's docs and
    // matching expressions), so naive co-occurrence would match
    // every entry against this file and falsely flag every allow-
    // list entry as stale.
    let self_file = workspace.join("crates/verter_session/tests/cases/architecture/lifecycle.rs");
    for crate_entry in crate_entries.flatten() {
        let tests_dir = crate_entry.path().join("tests");
        if !tests_dir.is_dir() {
            continue;
        }
        // Recurse: consolidated suites nest their TLS-propagation tests
        // in group subdirectories (e.g. `tests/cases/g_misc0/*tls*.rs`),
        // so a top-level-only scan would miss them and let a stale
        // `MISSING_TLS_TEST` entry pass undetected.
        for entry in walkdir::WalkDir::new(&tests_dir) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let p = entry.path().to_path_buf();
            if p.extension().is_some_and(|e| e == "rs") && p != self_file {
                tls_test_files.push(p);
            }
        }
    }
    let mut stale_missing: Vec<String> = Vec::new();
    for (symbol, rationale) in MISSING_TLS_TEST {
        // Reject entries whose symbol is also in WAVE_3_ENTRY_POINTS
        // (would be a contradiction: pinned + missing).
        if WAVE_3_ENTRY_POINTS.iter().any(|(s, _)| s == symbol) {
            stale_missing.push(format!(
                "  - {symbol}: present in BOTH WAVE_3_ENTRY_POINTS and MISSING_TLS_TEST. \
                 Remove from one list."
            ));
            continue;
        }
        for path in &tls_test_files {
            let src = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if src.contains(symbol) && src.contains("assert_observer_reaches") {
                stale_missing.push(format!(
                    "  - {symbol}: a TLS-propagation test now exists at {} \
                     (rationale was: {rationale}). Promote {symbol} to \
                     WAVE_3_ENTRY_POINTS and drop the MISSING_TLS_TEST entry.",
                    path.display(),
                ));
                break;
            }
        }
    }

    assert!(
        stale_missing.is_empty(),
        "wave_3_entry_points_propagate_tls: stale MISSING_TLS_TEST entries:\n{}",
        stale_missing.join("\n"),
    );
}

/// R22 / R3 — the upsert path performs NO eager cache drain, on
/// either the cross-file or the same-canonical axis.
///
/// `host_upsert.rs` must not, anywhere in its body, call
/// `reverse_deps_for`, `invalidate_canonical`, or `evict_canonical`.
///
/// Two retired drains map onto those identifiers:
///
/// - The reverse-dependent cascade. An owner upsert iterated
///   `ws().reverse_deps_for(canonical)` and
///   `resolver.runtime.invalidate_canonical(owner)`'d every dependent.
///   A downstream consumer's warm cache is now revalidated lazily on
///   read through its own `fact_dep_signature` check.
/// - The own-canonical drain. An upsert eagerly evicted the upserted
///   canonical's own query-identity caches —
///   `resolver.runtime.evict_canonical(&canonical_id)`,
///   `project_type_store.evict_canonical(&canonical_id)`. A warm
///   query-identity entry for
///   the upserted canonical is now rejected on the cold-recompute read
///   path by its current-content self-version root.
///
/// Same-canonical invalidation is lazy via self-version-rooted fact
/// validation; reintroducing either eager drain into `host_upsert.rs`
/// is forbidden.
#[test]
fn host_upsert_performs_no_reverse_dependent_eviction() {
    use syn::visit::Visit;

    let src = read_workspace_file("crates/verter_session/src/host_upsert.rs");
    let parsed = syn::parse_file(&src).expect("parse host_upsert.rs");
    let mut scanner = UpsertEagerDrainScanner::default();
    scanner.visit_file(&parsed);

    assert!(
        scanner.hits.is_empty(),
        "host_upsert.rs calls an eager cache-drain method ({:?}). \
         The upsert performs NO eager drain on either axis. Cross-file: \
         the reverse-dependent cascade is removed — `reverse_deps_for` / \
         `invalidate_canonical` must not reappear; cross-file consumers \
         revalidate lazily on read via `fact_dep_signature`. \
         Same-canonical: the own-canonical drain is removed — \
         `evict_canonical(&canonical_id)` \
         must not reappear; a warm query-identity entry for the upserted \
         canonical is rejected on the cold-recompute read path by its \
         current-content self-version root.",
        scanner.hits
    );
}

/// Discriminating self-test for
/// [`host_upsert_performs_no_reverse_dependent_eviction`]: the
/// [`UpsertEagerDrainScanner`] must FLAG both the reverse-dependent
/// cascade and the own-canonical drain, and must ACCEPT a bare
/// `.clear()` on an unrelated cache. Without this the production guard
/// could pass trivially.
#[test]
fn host_upsert_reverse_dep_eviction_scanner_discriminates() {
    use syn::visit::Visit;

    fn scan(src: &str) -> Vec<String> {
        let parsed = syn::parse_file(src).expect("parse fixture");
        let mut s = UpsertEagerDrainScanner::default();
        s.visit_file(&parsed);
        s.hits
    }

    // FORBIDDEN: the reverse-dependent cascade shape — flagged.
    let reverse_dep_fixture = r#"
        impl Host {
            fn upsert(&self) {
                for owner in self.ws().reverse_deps_for(&id) {
                    self.resolver.runtime.invalidate_canonical(owner);
                }
            }
        }
    "#;
    assert!(
        !scan(reverse_dep_fixture).is_empty(),
        "scanner must flag a reverse_deps_for / invalidate_canonical cascade"
    );

    // FORBIDDEN: the own-canonical drain shape — flagged. Reintroducing
    // `evict_canonical(&canonical_id)`
    // into the upsert path is banned: same-canonical invalidation is
    // lazy via self-version-rooted fact validation.
    let own_canonical_drain_fixture = r#"
        impl Host {
            fn upsert(&self) {
                self.resolver.runtime.evict_canonical(&canonical_id);
                self.project_type_store.evict_canonical(&canonical_id);
            }
        }
    "#;
    let drain_hits = scan(own_canonical_drain_fixture);
    assert!(
        drain_hits
            .iter()
            .filter(|h| *h == "evict_canonical")
            .count()
            == 2,
        "scanner must flag both `evict_canonical` calls, got {drain_hits:?}"
    );

    // ACCEPTED: a bare `.clear()` on an unrelated cache is not an
    // own-canonical drain — the per-domain compile/derived-cache field
    // resets the upsert legitimately performs must not be flagged.
    let unrelated_clear_fixture = r#"
        impl Host {
            fn upsert(&self) {
                profile.compile_slots.clear();
                derived.cached_resolved_meta.clear();
            }
        }
    "#;
    assert!(
        scan(unrelated_clear_fixture).is_empty(),
        "scanner must NOT flag a bare `.clear()` on an unrelated cache field"
    );
}

#[test]
fn resolver_store_view_into_owned_view_is_allowlisted() {
    // Part D — the raw-`HostStoreView` escape hatch
    // (`StoreViewRead::into_owned_view`) appears in production ONLY in the
    // allowlisted test-fixture / driver-snapshot / fenced-cold-seed
    // producers. A new production file that grabs a raw view (the seam a
    // future warm-validation regression would slip through) fails here and
    // must instead choose `.current()` (warm) or
    // `.into_cold_seed_view()` (fenced cold).
    let allow: std::collections::HashSet<&str> =
        INTO_OWNED_VIEW_ALLOWLIST.iter().copied().collect();
    let mut offenders: Vec<String> = Vec::new();
    for path in store_view_guard_production_rs_files() {
        let rel = rel_path(&path);
        if store_view_guard_is_test_file(&rel) {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        if src.contains(".into_owned_view()") && !allow.contains(rel.as_str()) {
            offenders.push(rel);
        }
    }
    assert!(
        offenders.is_empty(),
        "`StoreViewRead::into_owned_view()` (the raw-`HostStoreView` escape hatch) \
         is confined to the test-fixture / driver-snapshot / fenced-cold-seed \
         allowlist. A new production caller must choose `.current()` (warm \
         validation) or `.into_cold_seed_view()` (fenced cold builder), not the \
         raw owned view. Offending files:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn cold_seed_into_inner_confined_to_non_validating_allowlist() {
    // The cold-seed raw-unwrap escape hatch
    // (`ColdSeedHostStoreView::into_inner` via
    // `.into_cold_seed_view().into_inner()`) DROPS the seed's `is_current`
    // flag. It appears in production ONLY in the non-validating allowlist
    // (driver-snapshot accessors + `#[cfg(test)]` direct-host wrappers).
    //
    // This is the INDIRECT-validation seam the earlier capability-split
    // guard missed: a raw cold-seed view fed into a resolver context
    // (`HostResolverContext::new` / `SessionResolverContext::new`) whose
    // nested `validates*` family then validated a warm-cache entry against
    // a stale seed. A new cold-compute path that unwraps a cold-seed must
    // instead carry the currentness (`into_cold_seed_view` straight into
    // `with_session_overlay` / `from_cold_seed`, or the executor-boundary
    // re-bind `from_executor_snapshot`).
    let allow: std::collections::HashSet<&str> =
        COLD_SEED_INTO_INNER_ALLOWLIST.iter().copied().collect();
    let mut offenders: Vec<String> = Vec::new();
    for path in store_view_guard_production_rs_files() {
        let rel = rel_path(&path);
        if store_view_guard_is_test_file(&rel) {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        if contains_cold_seed_into_inner(&src) && !allow.contains(rel.as_str()) {
            offenders.push(rel);
        }
    }
    assert!(
        offenders.is_empty(),
        "`ColdSeedHostStoreView::into_inner()` (the `.into_cold_seed_view().into_inner()` \
         raw-unwrap that DROPS the seed's `is_current` flag) is confined to the \
         non-validating driver-snapshot / `#[cfg(test)]`-wrapper allowlist. A new \
         cold-compute path that unwraps a cold-seed and feeds the raw view into a \
         resolver context performing nested warm-cache validation MUST instead \
         preserve currentness — `StoreViewRead::into_cold_seed_view` straight into \
         `with_session_overlay` + `*ResolverContext::from_cold_seed`, or the \
         executor-boundary re-bind `StoreViewRead::from_executor_snapshot(view, is_current)` \
         — so a `ReturnOnly` seed fails the context's `validates*` family closed. Offending \
         files:\n  {}",
        offenders.join("\n  ")
    );
}

/// The `wasm32` decl-lowering path must RETAIN its parse snapshot in a
/// single-thread thread-local shard — NOT a `DeclLoweringService` field
/// (the `Rc`-backed parse is `!Send`/`!Sync`; a field would poison the
/// service's `Send + Sync` bounds), and NOT an inline reparse per
/// demand. Guards against both the documentation-only exemption and the
/// FIX1C `RefCell<SnapshotShard>` service field that broke the wasm
/// build.
#[test]
fn decl_lowering_wasm_path_retains_snapshot_source_guard() {
    let src = read_workspace_file("crates/verter_semantic_source/src/decl_lowering.rs");

    // Stale "no retention" / "parse inline per call" wording is gone.
    for stale in ["parse inline per call", "No retention", "no retention"] {
        assert!(
            !src.contains(stale),
            "decl_lowering.rs still carries the stale wasm wording `{stale}` — \
             the wasm path now retains a snapshot shard, not an inline reparse"
        );
    }

    // The wasm retention lives in a thread-local shard, never a service
    // field. A `RefCell<SnapshotShard>` field on the service is the
    // FIX1C regression that poisoned `Send + Sync` on wasm.
    assert!(
        src.contains("thread_local!") && src.contains("WASM_DECL_LOWERING_SHARD"),
        "the wasm decl-lowering path must retain via a `thread_local!` \
         `WASM_DECL_LOWERING_SHARD` shard"
    );

    // No `unsafe impl Send`/`Sync` papering over the `!Send` parse.
    for forbidden in [
        "unsafe impl Send for DeclLoweringService",
        "unsafe impl Sync for DeclLoweringService",
    ] {
        assert!(
            !src.contains(forbidden),
            "decl_lowering.rs must not paper over the `!Send` parse with \
             `{forbidden}` — the wasm shard is a thread-local instead"
        );
    }

    // Isolate the `target_arch = \"wasm32\"` arm of `run(...)` and prove it
    // routes through the retained thread-local shard rather than
    // unconditionally reporting a fresh parse.
    let wasm_run_marker = "// Single-threaded platform:";
    let wasm_run = src
        .split(wasm_run_marker)
        .nth(1)
        .expect("the wasm `run` arm must carry its explanatory comment");
    let wasm_run = &wasm_run[..wasm_run.len().min(800)];
    assert!(
        wasm_run.contains("snapshot_for_run"),
        "the wasm `run` arm must reuse the retained shard via \
         `snapshot_for_run`"
    );
    assert!(
        wasm_run.contains("WASM_DECL_LOWERING_SHARD"),
        "the wasm `run` arm must reuse the thread-local retained shard"
    );
    assert!(
        !wasm_run.contains("parsed_now: true"),
        "the wasm `run` arm must derive `parsed_now` from the shard's \
         hit/miss result — never an unconditional `parsed_now: true`"
    );
}
