//! Architecture guard: editor-liveness + close-after-successful-sync.
//!
//! CRITICAL rule (see `CLAUDE.md` / `.claude/skills/host-session`): every
//! `.vue` IDE/API provider sync MUST go through the shared per-kind
//! close-AFTER-successful-sync discipline. A function that syncs a `.vue`
//! artifact must NEVER bulk-close a transition's stale set inline (close-
//! before-sync), because that closes the live editor TSX on an owner change
//! and leaves the prior path closed when the replacement sync fails.
//!
//! A grep-based whole-class audit MISSED residual close-before-sync paths
//! TWICE (the `_in_background` API twin, the SyncCoordinator debounced loop,
//! and the workspace-scanner background sync). This guard makes a third miss
//! mechanically impossible: it source-scans every LSP provider-sync file and
//! FAILS if any function OTHER THAN the approved leaf close-dispatch
//! primitives contains an inline provider-close loop.
//!
//! The single approved shape is: build `genuinely_stale` via
//! `genuinely_stale_after_sync(..)` AFTER a successful per-kind sync, then hand
//! that already-filtered slice to ONE of the leaf close-dispatch primitives.
//! The leaf primitives (and ONLY they) iterate a path slice and call
//! `close_tsx` / `close_dts` / `close_file`; every other function delegates.
//!
//! Discriminates: against the pre-R2 tree where
//! `sync_api_to_provider_in_background` (and the SyncCoordinator / scanner sync
//! bodies) contained an inline `for (kind, path) in &transition.stale_paths {
//! .. close_tsx/close_dts .. }` loop, this guard FAILS (those functions are not
//! in the allow-list). After routing them through the shared discipline it
//! PASSES (only the named leaf primitives keep an inline close loop).

use std::collections::BTreeSet;
use std::path::PathBuf;

/// LSP provider-sync source files that perform `.vue` IDE/API sync.
const SYNC_SOURCE_FILES: &[&str] = &[
    "src/background_drain.rs",
    "src/server/sync_orchestration.rs",
    "src/server/provider_state.rs",
    "src/sync_coordinator.rs",
    "src/workspace_scanner.rs",
];

/// The ONLY functions permitted to contain an inline provider-close loop
/// (`for ... { close_tsx/close_dts/close_file }`). Each is a leaf close-dispatch
/// primitive that iterates an ALREADY-FILTERED `genuinely_stale` slice handed to
/// it by a caller that gated through `genuinely_stale_after_sync`.
const APPROVED_CLOSE_DISPATCH_PRIMITIVES: &[&str] = &[
    "close_stale_provider_paths", // background_drain.rs
    "close_provider_paths",       // server/provider_state.rs
    "close_stale_paths",          // sync_coordinator.rs + workspace_scanner.rs (private leaf)
];

/// The leaf close-dispatch helpers whose CALL SITES are audited for a raw
/// transition-stale-set argument (the delegated close-before-sync evasion).
const CLOSE_DISPATCH_HELPERS: &[&str] = &[
    "close_stale_provider_paths",
    "close_provider_paths",
    "close_stale_paths",
    "close_dropped_owner_api_path",
];

/// `.vue` IDE/API provider primitives. A function that calls any of these
/// produces a `.vue` IDE/API artifact and is therefore bound by the editor-
/// liveness invariant — the NON-Vue (`Shadow`) sync functions use the `*_file`
/// primitives instead and are intentionally outside the invariant.
const VUE_IDE_API_PRIMITIVES: &[&str] = &[
    "sync_tsx",
    "open_tsx",
    "close_tsx",
    "sync_dts",
    "open_dts",
    "close_dts",
];

fn crate_src_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Strip the trailing `#[cfg(test)] mod tests { .. }` (and anything after the
/// first `#[cfg(test)]`) so the guard scans production source only.
fn production_only(src: &str) -> String {
    match src.find("\n#[cfg(test)]") {
        Some(idx) => src[..idx].to_string(),
        None => src.to_string(),
    }
}

/// True if `line` opens a `for` loop.
fn is_for_loop_header(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("for ") || t == "for"
}

/// True if `line` issues a provider close on an IDE/API path.
fn closes_ide_or_api_path(line: &str) -> bool {
    line.contains("close_tsx(") || line.contains("close_dts(")
}

/// Walk a production source string and return the set of `fn` names whose body
/// contains a `for` loop that (within the loop's first lines) closes an IDE/API
/// provider path. Function attribution uses the nearest preceding `fn <name>(`.
fn functions_with_inline_close_loop(src: &str) -> BTreeSet<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut offenders = BTreeSet::new();

    // Precompute, for each line, the nearest preceding `fn <name>` declaration.
    let mut current_fn: Option<String> = None;
    let mut fn_at_line: Vec<Option<String>> = Vec::with_capacity(lines.len());
    for line in &lines {
        if let Some(name) = extract_fn_name(line) {
            current_fn = Some(name);
        }
        fn_at_line.push(current_fn.clone());
    }

    for (i, line) in lines.iter().enumerate() {
        if !is_for_loop_header(line) {
            continue;
        }
        // Scan the loop body window for a provider close. A close-dispatch loop
        // is short; 12 lines comfortably covers the `match kind { .. }` arm
        // block while staying inside the loop.
        let window_end = (i + 12).min(lines.len());
        let body_closes = lines[i + 1..window_end]
            .iter()
            .any(|l| closes_ide_or_api_path(l));
        if body_closes {
            if let Some(Some(name)) = fn_at_line.get(i) {
                offenders.insert(name.clone());
            }
        }
    }
    offenders
}

/// Extract the function name from a `fn <name>(` / `pub(..) async fn <name>(`
/// line, if present.
fn extract_fn_name(line: &str) -> Option<String> {
    let idx = line.find("fn ")?;
    // Reject substrings like `..fn ` embedded in identifiers by requiring the
    // `fn ` token to be preceded by start-of-line whitespace or a keyword.
    let prefix = line[..idx].trim();
    let ok_prefix = prefix.is_empty()
        || prefix.ends_with("pub")
        || prefix.ends_with("async")
        || prefix.ends_with(')') // pub(super) / pub(crate)
        || prefix.ends_with("const")
        || prefix.ends_with("unsafe");
    if !ok_prefix {
        return None;
    }
    let rest = &line[idx + 3..];
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// AST-walk a function body and decide whether it delegates a RAW transition
/// stale set to a close-dispatch helper while ALSO being a `.vue`-syncing
/// function. This is the syn-based replacement for the prior line-based scan,
/// which only caught the same-line `helper(&transition.stale_paths)` form and
/// MISSED three evasions:
///
///   * **multiline call spans** — `close_provider_paths(\n  &transition\n    .stale_paths,\n)`;
///   * **bare-local stale sets** — `let stale_paths = transition.stale_paths;
///     close_stale_paths(&stale_paths)` (no `.stale_paths` on the call line);
///   * **aliased delegation** — `let raw = transition.stale_paths;
///     close_stale_paths(&raw)` (the local name carries no `stale_paths` token).
///
/// Parsing the AST collapses all three to the same structural fact: an argument
/// of a close-dispatch-helper call resolves to a transition's raw `.stale_paths`
/// field (directly, or transitively through a `let`-bound local) WITHOUT having
/// been routed through `genuinely_stale_after_sync(..)`. The approved shape — a
/// local bound from `genuinely_stale_after_sync(..)` handed to the close helper
/// — is structurally distinct (its initializer contains the filter call), so it
/// is never flagged.
struct DelegatedCloseDetector {
    /// Locals whose initializer reads a transition's raw `.stale_paths` and was
    /// NOT routed through `genuinely_stale_after_sync` (raw aliases).
    raw_stale_locals: BTreeSet<String>,
    /// Whether the function references any `.vue` IDE/API primitive.
    is_vue_syncing: bool,
    /// Whether the function delegates a raw stale set to a close helper.
    delegates_raw_stale: bool,
}

impl DelegatedCloseDetector {
    fn analyze(block: &syn::Block) -> Self {
        // First: collect raw-stale `let`-bound locals to a FIXED POINT. A local
        // is raw-stale when its initializer carries raw `.stale_paths` taint
        // (a `.stale_paths` field read, OR a reference to an already-raw-stale
        // local) and was NOT routed through `genuinely_stale_after_sync`. The
        // fixed point propagates taint through multi-hop alias chains
        // (`let a = transition.stale_paths; let b = a.clone();` taints both) and
        // through `Vec`-collected-from-a-tainted-local
        // (`let v = raw.iter().cloned().collect();` taints `v`). One pass marks
        // direct-field locals; subsequent passes mark locals whose initializer
        // reads a now-tainted local; iteration stops when a pass adds nothing.
        let mut raw_stale_locals = BTreeSet::new();
        loop {
            let mut collector = RawStaleBindingCollector {
                raw_stale_locals: raw_stale_locals.clone(),
            };
            syn::visit::visit_block(&mut collector, block);
            if collector.raw_stale_locals.len() == raw_stale_locals.len() {
                // No new taint discovered this pass: fixed point reached.
                raw_stale_locals = collector.raw_stale_locals;
                break;
            }
            raw_stale_locals = collector.raw_stale_locals;
        }
        // Then: classify primitive usage + close-helper delegation against the
        // settled taint set.
        let mut detector = DelegatedCloseDetector {
            raw_stale_locals,
            is_vue_syncing: false,
            delegates_raw_stale: false,
        };
        syn::visit::visit_block(&mut detector, block);
        detector
    }
}

/// The single ROOT-TAINT rule. Walk an expression tree (the full `syn` subtree,
/// descending through references, parens/groups, named field access, index /
/// slice, and method-call receivers) and report whether it is ROOTED in raw
/// `.stale_paths` taint, given the set of locals already known to be raw-stale.
/// Taint is:
///
///   * a `.stale_paths` NAMED field access anywhere in the tree — covering the
///     bare field (`transition.stale_paths`), references (`&t.stale_paths`),
///     wrappers (`t.stale_paths.clone()`,
///     `t.stale_paths.iter().cloned().collect()`), and a field nested under a
///     projection (`Holder { paths: t.stale_paths }`, `t.stale_paths[..]`); OR
///   * a path reference to a local in `tainted_locals` anywhere in the tree —
///     covering the bare local (`raw`), references (`&raw`), wrappers
///     (`raw.clone()`, `raw.iter()...collect()`), index/slice (`&raw[..]`),
///     method-receiver projections (`raw.as_slice()`, `&raw.to_owned()`), and a
///     tainted local stored into a struct field then projected (`&h.paths`,
///     where `h` was tainted by storing `raw`).
///
/// Because taint is detected ANYWHERE in the subtree, a projection / wrapper /
/// aggregate whose ROOT is a tainted local or raw field is tainted — there is no
/// per-form special-case; one walk subsumes the struct-field, index/slice, and
/// method-receiver-projection forms. This is strictly conservative: it never has
/// MORE false negatives than a literal walk-to-the-single-root would.
///
/// The FILTER EXEMPTION: a `genuinely_stale_after_sync(..)` call CONSUMES its
/// raw stale argument and produces a filtered result, so the finder does NOT
/// descend into such a call's subtree. A `.stale_paths` access (or tainted
/// local) that appears ONLY as an argument to `genuinely_stale_after_sync` is
/// therefore filtered, not raw — and a PROJECTION of the filtered result
/// (`&genuinely_stale[..]`, `genuinely_stale.as_slice()`) stays filtered too,
/// because the only taint sources inside it are behind the filter boundary. This
/// one definition drives BOTH `let`-taint propagation and the recursive
/// close-arg inspection, so they cannot diverge.
struct RawStaleFinder<'a> {
    tainted_locals: &'a BTreeSet<String>,
    found: bool,
}

impl<'a, 'ast> syn::visit::Visit<'ast> for RawStaleFinder<'a> {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        // Do NOT descend into a `genuinely_stale_after_sync(..)` call: its
        // arguments are filtered, so any `.stale_paths` / tainted local inside
        // it is not raw.
        if call_name_is(call.func.as_ref(), &["genuinely_stale_after_sync"]) {
            return;
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        // `.genuinely_stale_after_sync(..)` is never written as a method, but be
        // robust: a method named so would likewise be a filtered boundary.
        if mc.method == "genuinely_stale_after_sync" {
            return;
        }
        syn::visit::visit_expr_method_call(self, mc);
    }

    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if let syn::Member::Named(ident) = &field.member {
            if ident == "stale_paths" {
                self.found = true;
            }
        }
        syn::visit::visit_expr_field(self, field);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if let Some(id) = path.path.get_ident() {
            if self.tainted_locals.contains(&id.to_string()) {
                self.found = true;
            }
        }
        syn::visit::visit_expr_path(self, path);
    }
}

/// True if `expr`'s tree carries raw `.stale_paths` taint (see [`RawStaleFinder`]),
/// given the locals already known to be raw-stale.
fn expr_carries_raw_stale(expr: &syn::Expr, tainted_locals: &BTreeSet<String>) -> bool {
    let mut finder = RawStaleFinder {
        tainted_locals,
        found: false,
    };
    syn::visit::visit_expr(&mut finder, expr);
    finder.found
}

/// Collects `let`-bound locals whose initializer carries raw `.stale_paths`
/// taint (a `.stale_paths` field read OR a reference to an already-raw-stale
/// local), skipping any `genuinely_stale_after_sync(..)`-filtered subtree.
///
/// Seeded with the current taint set and re-run to a fixed point by
/// [`DelegatedCloseDetector::analyze`], so multi-hop alias chains,
/// `Vec`-collected-from-a-tainted-local forms, AND struct-construction /
/// aggregate forms that STORE a tainted value all propagate. `let stale_paths =
/// transition.stale_paths;`, `let raw = transition.stale_paths.clone();`,
/// `let b = a.clone();` (when `a` is tainted), `let v =
/// raw.iter().cloned().collect();` (when `raw` is tainted), and `let h = Holder
/// { paths: a };` / `let h = Holder { paths: transition.stale_paths };` (the
/// initializer is a struct literal whose subtree roots in a tainted local or the
/// raw field — [`expr_carries_raw_stale`] sees it) all qualify; `let
/// genuinely_stale = genuinely_stale_after_sync(&stale_paths, ..)` does not (its
/// initializer's only `.stale_paths` is inside the filter call). Because the
/// initializer check reuses [`expr_carries_raw_stale`], propagation through
/// aggregates needs no separate rule: storing a tainted value into a struct/Vec
/// taints the binding, and projecting that binding back out
/// (`&h.paths`, `v.as_slice()`) re-roots in the tainted local.
struct RawStaleBindingCollector {
    raw_stale_locals: BTreeSet<String>,
}

/// Extract the bound identifier from a `let` pattern, unwrapping a type
/// annotation. `let v = ..` is `Pat::Ident`; `let v: Vec<_> = ..` is
/// `Pat::Type { pat: Pat::Ident, .. }`. Both bind `v`; a `Vec`-collected raw
/// stale set is almost always type-annotated, so the type form MUST be handled
/// or `Vec`-collect taint silently escapes.
fn let_binding_ident(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(pat_ident) => Some(pat_ident.ident.to_string()),
        syn::Pat::Type(pat_type) => let_binding_ident(pat_type.pat.as_ref()),
        _ => None,
    }
}

impl<'ast> syn::visit::Visit<'ast> for RawStaleBindingCollector {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (Some(ident), Some(init)) = (let_binding_ident(&local.pat), &local.init) {
            if expr_carries_raw_stale(init.expr.as_ref(), &self.raw_stale_locals) {
                self.raw_stale_locals.insert(ident);
            }
        }
        syn::visit::visit_local(self, local);
    }
}

/// True if the terminal path segment of a call/method-call name is one of
/// `names`.
fn call_name_is(func: &syn::Expr, names: &[&str]) -> bool {
    if let syn::Expr::Path(p) = func {
        if let Some(last) = p.path.segments.last() {
            return names.contains(&last.ident.to_string().as_str());
        }
    }
    false
}

impl<'ast> syn::visit::Visit<'ast> for DelegatedCloseDetector {
    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        let method = mc.method.to_string();
        // `.vue` IDE/API primitive used as a method (`sync.sync_tsx(..)`).
        if VUE_IDE_API_PRIMITIVES.contains(&method.as_str()) {
            self.is_vue_syncing = true;
        }
        // A close-dispatch helper invoked as a method (`self.close_provider_paths(..)`).
        if CLOSE_DISPATCH_HELPERS.contains(&method.as_str()) {
            self.check_close_args(mc.args.iter());
        }
        syn::visit::visit_expr_method_call(self, mc);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        // `.vue` IDE/API primitive used as a free/associated call.
        if call_name_is(call.func.as_ref(), VUE_IDE_API_PRIMITIVES) {
            self.is_vue_syncing = true;
        }
        // A close-dispatch helper invoked as a free fn
        // (`close_stale_provider_paths(sync, &transition.stale_paths, ctx)`).
        if call_name_is(call.func.as_ref(), CLOSE_DISPATCH_HELPERS) {
            self.check_close_args(call.args.iter());
        }
        syn::visit::visit_expr_call(self, call);
    }
}

impl DelegatedCloseDetector {
    /// Flag a delegation if any close-helper argument's expression tree carries
    /// raw `.stale_paths` taint ANYWHERE in it: a direct `.stale_paths` field
    /// access, a `.stale_paths.clone()` / by-value wrapper, a reference to a
    /// raw-stale `let`-bound local (including multi-hop / `Vec`-collected
    /// aliases), or such a sub-expression nested arbitrarily deep in the arg.
    /// A `genuinely_stale_after_sync(..)`-filtered slice is exempt — the finder
    /// does not descend into the filter call — so `&genuinely_stale` and
    /// `&genuinely_stale.clone()` are allowed.
    fn check_close_args<'a>(&mut self, args: impl Iterator<Item = &'a syn::Expr>) {
        for arg in args {
            if expr_carries_raw_stale(arg, &self.raw_stale_locals) {
                self.delegates_raw_stale = true;
            }
        }
    }
}

/// AST-parse `src` and return the set of `.vue`-syncing `fn`/method names that
/// delegate a RAW transition stale set to a close-dispatch helper (the delegated
/// close-before-sync evasion of the inline-loop guard).
///
/// A function is `.vue`-syncing iff it references a `.vue` IDE/API primitive
/// ([`VUE_IDE_API_PRIMITIVES`]); the NON-Vue (`Shadow`) sync functions use the
/// `*_file` primitives and pass `transition.stale_paths` to a close helper
/// legitimately (they never produce a `.vue` IDE/API artifact), so they are not
/// bound by the editor-liveness invariant and are NOT flagged. The leaf
/// close-dispatch primitives themselves call `close_tsx`/`close_dts` DIRECTLY
/// (never another close-dispatch helper), so they never delegate and are never
/// flagged here.
fn vue_functions_delegating_raw_stale_close(src: &str) -> BTreeSet<String> {
    let file = match syn::parse_file(src) {
        Ok(f) => f,
        // A simulated-source snippet that does not parse as a full file (e.g. a
        // bare `fn` without `use`s) is handled by wrapping callers; production
        // source always parses.
        Err(_) => return BTreeSet::new(),
    };
    let mut offenders = BTreeSet::new();
    collect_delegating_fns_from_items(&file.items, &mut offenders);
    offenders
}

/// Walk every `fn` / impl-method / nested-module item and record the names of
/// `.vue`-syncing functions that delegate a raw stale set.
fn collect_delegating_fns_from_items(items: &[syn::Item], offenders: &mut BTreeSet<String>) {
    for item in items {
        match item {
            syn::Item::Fn(f) => {
                check_fn_block(&f.sig.ident.to_string(), &f.block, offenders);
            }
            syn::Item::Impl(imp) => {
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(m) = impl_item {
                        check_fn_block(&m.sig.ident.to_string(), &m.block, offenders);
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, sub_items)) = &m.content {
                    collect_delegating_fns_from_items(sub_items, offenders);
                }
            }
            _ => {}
        }
    }
}

fn check_fn_block(name: &str, block: &syn::Block, offenders: &mut BTreeSet<String>) {
    let analysis = DelegatedCloseDetector::analyze(block);
    if analysis.is_vue_syncing && analysis.delegates_raw_stale {
        offenders.insert(name.to_string());
    }
}

/// Parse a single-function snippet for the meta-guard. The meta-guard's fixtures
/// are individual `fn` items (not full files); `syn::parse_file` accepts a
/// sequence of items, so a bare `fn { .. }` parses as a one-item file. Returns
/// the flagged set.
fn vue_functions_delegating_raw_stale_close_snippet(src: &str) -> BTreeSet<String> {
    vue_functions_delegating_raw_stale_close(src)
}

#[test]
fn vue_sync_functions_never_inline_close_the_stale_set() {
    let approved: BTreeSet<String> = APPROVED_CLOSE_DISPATCH_PRIMITIVES
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut violations: Vec<String> = Vec::new();
    for rel in SYNC_SOURCE_FILES {
        let path = crate_src_path(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("guard must read {}: {e}", path.display()));
        let prod = production_only(&src);
        for offender in functions_with_inline_close_loop(&prod) {
            if !approved.contains(&offender) {
                violations.push(format!("{rel}::{offender}"));
            }
        }
    }
    violations.sort();

    assert!(
        violations.is_empty(),
        "editor-liveness guard: these `.vue` sync functions bulk-close a stale \
         path set inline (close-before-sync), which closes the live editor TSX \
         on an owner change and loses the prior path on a failed sync.\n\n  {}\n\n\
         Route stale-path closing through the shared close-AFTER-successful-sync \
         discipline: build `genuinely_stale` via `genuinely_stale_after_sync(..)` \
         AFTER a successful per-kind sync, then hand it to one of the approved \
         leaf close-dispatch primitives ({}). Only those leaf primitives may \
         contain an inline provider-close loop.",
        violations.join("\n  "),
        APPROVED_CLOSE_DISPATCH_PRIMITIVES.join(", "),
    );
}

#[test]
fn vue_sync_functions_never_delegate_raw_stale_close() {
    // R3-6: the inline-loop guard alone is EVADABLE — a `.vue`-syncing function
    // could hand the RAW `transition.stale_paths` to a leaf close-dispatch helper
    // BEFORE syncing (e.g. `close_provider_paths(&transition.stale_paths).await`),
    // which has no inline `for { close_tsx }` loop and so slips past
    // `functions_with_inline_close_loop`. That delegated close-before-sync closes
    // the live editor TSX on an owner change and loses the prior path on a failed
    // sync, exactly the class the inline-loop guard forbids.
    //
    // This guard closes that gap: a `.vue`-syncing function (one that touches a
    // `.vue` IDE/API primitive) must NEVER pass a raw transition stale set to a
    // close helper. The only approved shape hands the
    // `genuinely_stale_after_sync(..)`-filtered slice to the close helper AFTER a
    // successful per-kind sync. The NON-Vue (`Shadow`) sync functions
    // (`sync_non_vue_*`, the barrel non-vue pass) legitimately close
    // `transition.stale_paths` — they never produce a `.vue` IDE/API artifact and
    // are outside the editor-liveness invariant, so they are not `.vue`-syncing
    // and are not flagged.
    let mut violations: Vec<String> = Vec::new();
    for rel in SYNC_SOURCE_FILES {
        let path = crate_src_path(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("guard must read {}: {e}", path.display()));
        let prod = production_only(&src);
        for offender in vue_functions_delegating_raw_stale_close(&prod) {
            violations.push(format!("{rel}::{offender}"));
        }
    }
    violations.sort();

    assert!(
        violations.is_empty(),
        "editor-liveness guard: these `.vue` sync functions delegate a RAW \
         transition stale set to a close helper (a delegated close-before-sync \
         that evades the inline-loop guard), which closes the live editor TSX on \
         an owner change and loses the prior path on a failed sync.\n\n  {}\n\n\
         Route stale-path closing through close-AFTER-successful-sync: build \
         `genuinely_stale` via `genuinely_stale_after_sync(..)` AFTER a successful \
         per-kind sync, then hand that filtered slice (NOT `transition.stale_paths`) \
         to a leaf close-dispatch primitive.",
        violations.join("\n  "),
    );
}

/// Meta-guard: the detector itself must be discriminating — it must FLAG the
/// pre-R2 inline-stale-close idiom and must NOT flag the approved delegated
/// shape. Pins the guard so a future weakening (e.g. widening the window to
/// zero, or matching nothing) is caught.
#[test]
fn guard_detector_discriminates_inline_close_from_delegation() {
    // The exact pre-R2 anti-pattern from `sync_api_to_provider_in_background`.
    let buggy = r#"
pub(super) async fn sync_api_to_provider_background_task(sync: ProjectSync) {
    for (kind, path) in &transition.stale_paths {
        let result = match kind {
            ProviderPathKind::Ide => sync.close_tsx(path).await,
            ProviderPathKind::Api => sync.close_dts(path).await,
            ProviderPathKind::Shadow => sync.close_file(path).await,
        };
    }
    let _ = sync.open_dts(&dts_path, &api.code).await;
}
"#;
    let flagged = functions_with_inline_close_loop(buggy);
    assert!(
        flagged.contains("sync_api_to_provider_background_task"),
        "detector must FLAG an inline stale-close loop in a syncing fn, flagged={flagged:?}"
    );

    // The approved delegated shape: build `genuinely_stale`, hand it to a leaf
    // primitive. The syncing fn itself has NO inline close loop.
    let delegated = r#"
async fn sync_owner_resolved_vue_with_close_after_sync(sync: ProjectSync) {
    let result = sync.open_tsx(&ide_path, ide_code).await;
    let genuinely_stale = genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
    commit_sync_transition(provider_sync_states, canonical_id, committed_state);
    close_stale_provider_paths(sync, &genuinely_stale, context).await;
}

pub(super) async fn close_stale_provider_paths(sync: &ProjectSync, stale_paths: &[(K, String)]) {
    for (kind, path) in stale_paths {
        let result = match kind {
            ProviderPathKind::Ide => sync.close_tsx(path).await,
            ProviderPathKind::Api => sync.close_dts(path).await,
            ProviderPathKind::Shadow => sync.close_file(path).await,
        };
    }
}
"#;
    let flagged = functions_with_inline_close_loop(delegated);
    assert!(
        !flagged.contains("sync_owner_resolved_vue_with_close_after_sync"),
        "detector must NOT flag a fn that delegates to a leaf primitive, flagged={flagged:?}"
    );
    assert!(
        flagged.contains("close_stale_provider_paths"),
        "detector must flag the leaf primitive's own loop (it is allow-listed by name), flagged={flagged:?}"
    );
}

/// Meta-guard for the R3-6 delegated-close detector: it must FLAG a simulated
/// `.vue`-syncing function that hands the RAW `transition.stale_paths` to a
/// close helper before syncing (the evasion), must NOT flag the approved shape
/// (filtered `genuinely_stale` slice), and must NOT flag a NON-Vue (`Shadow`)
/// sync function that legitimately closes `transition.stale_paths` (outside the
/// editor-liveness invariant). Pins the detector so a future weakening is caught.
#[test]
fn delegated_close_detector_discriminates_vue_evasion_from_approved_and_non_vue() {
    // EVASION: a `.vue`-syncing fn (it calls `sync_tsx`) that hands the RAW
    // transition stale set to a close helper BEFORE syncing — no inline
    // `for { close_tsx }` loop, so the inline-loop guard misses it.
    let evasion = r#"
pub(super) async fn sync_owner_resolved_vue_evasion(sync: ProjectSync) {
    self.close_provider_paths(&transition.stale_paths).await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged = vue_functions_delegating_raw_stale_close(evasion);
    assert!(
        flagged.contains("sync_owner_resolved_vue_evasion"),
        "detector must FLAG a `.vue`-syncing fn that delegates a raw stale set to a close helper, flagged={flagged:?}"
    );

    // Also flag the other delegated close-helper shapes a `.vue` sync fn might use.
    let evasion2 = r#"
async fn resync_vue_evasion(sync: ProjectSync) {
    close_stale_provider_paths(sync, &transition.stale_paths, "ctx").await;
    let _ = sync.sync_dts(&dts_path, &api.code).await;
}
"#;
    let flagged2 = vue_functions_delegating_raw_stale_close(evasion2);
    assert!(
        flagged2.contains("resync_vue_evasion"),
        "detector must FLAG `close_stale_provider_paths(&transition.stale_paths)` in a `.vue` sync fn, flagged={flagged2:?}"
    );

    // APPROVED: a `.vue`-syncing fn that hands the FILTERED `genuinely_stale`
    // slice to the close helper AFTER a successful sync — must NOT flag.
    let approved = r#"
async fn sync_owner_resolved_vue_with_close_after_sync(sync: ProjectSync) {
    let result = sync.open_tsx(&ide_path, ide_code).await;
    let stale_paths = transition.stale_paths;
    let genuinely_stale = genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
    close_stale_provider_paths(sync, &genuinely_stale, context).await;
}
"#;
    let flagged_approved = vue_functions_delegating_raw_stale_close(approved);
    assert!(
        !flagged_approved.contains("sync_owner_resolved_vue_with_close_after_sync"),
        "detector must NOT flag the approved `genuinely_stale` close shape, flagged={flagged_approved:?}"
    );

    // NON-VUE (out of scope): a Shadow sync fn that closes `transition.stale_paths`
    // — it uses `*_file` (no `.vue` IDE/API primitive), so it is NOT `.vue`-syncing
    // and must NOT be flagged.
    let non_vue = r#"
pub(super) async fn sync_non_vue_file_to_provider(sync: ProjectSync) {
    self.close_provider_paths(&transition.stale_paths).await;
    let _ = sync.open_file(&shadow_path, &code).await;
}
"#;
    let flagged_non_vue = vue_functions_delegating_raw_stale_close(non_vue);
    assert!(
        flagged_non_vue.is_empty(),
        "detector must NOT flag a NON-Vue Shadow sync fn closing transition.stale_paths, flagged={flagged_non_vue:?}"
    );

    // The leaf primitive's OWN definition (`fn close_stale_paths(stale_paths: ..)`)
    // iterates `stale_paths` but is not a CALL site — must NOT be flagged even
    // though it touches close_tsx/close_dts.
    let leaf = r#"
async fn close_stale_paths(sync: &ProjectSync, stale_paths: &[(K, String)]) {
    for (kind, path) in stale_paths {
        let _ = sync.close_tsx(path).await;
    }
}
"#;
    let flagged_leaf = vue_functions_delegating_raw_stale_close(leaf);
    assert!(
        flagged_leaf.is_empty(),
        "detector must NOT flag a leaf primitive's own signature/loop (not a call site), flagged={flagged_leaf:?}"
    );

    // ── R5-3: the three evasion forms the prior LINE-BASED detector MISSED ──

    // EVASION (multiline): the raw `transition.stale_paths` argument is on a
    // DIFFERENT line from the close-helper call. The prior detector examined
    // only the call line's tail (empty here), so it slipped past. The AST
    // detector sees the full call regardless of line breaks.
    let evasion_multiline = r#"
pub(super) async fn sync_vue_multiline_evasion(sync: ProjectSync) {
    close_provider_paths(
        &transition.stale_paths,
    )
    .await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_ml = vue_functions_delegating_raw_stale_close_snippet(evasion_multiline);
    assert!(
        flagged_ml.contains("sync_vue_multiline_evasion"),
        "detector must FLAG a multiline `close_provider_paths(\\n  &transition.stale_paths,\\n)` \
         delegation in a `.vue` sync fn, flagged={flagged_ml:?}"
    );

    // EVASION (bare local, by value): the raw set is bound to a local named
    // `stale_paths` and passed BY VALUE (no `&`) to the close helper before
    // filtering. The prior detector matched only the `&stale_paths` (referenced)
    // substring on the call line, so the by-value `stale_paths)` form slipped
    // past. The AST detector catches it structurally (the local's initializer
    // reads `transition.stale_paths` and was not filtered).
    let evasion_bare = r#"
async fn sync_vue_bare_local_evasion(sync: ProjectSync) {
    let stale_paths = transition.stale_paths;
    close_stale_paths(sync, stale_paths).await;
    let _ = sync.sync_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_bare = vue_functions_delegating_raw_stale_close_snippet(evasion_bare);
    assert!(
        flagged_bare.contains("sync_vue_bare_local_evasion"),
        "detector must FLAG a bare-local by-value `let stale_paths = transition.stale_paths; \
         close_stale_paths(stale_paths)` delegation, flagged={flagged_bare:?}"
    );

    // EVASION (alias): the raw set is bound to an ARBITRARILY-NAMED local
    // (`raw`) — its name carries no `stale_paths` token at all — and passed to
    // the close helper. The prior substring detector could NOT see this (no
    // `.stale_paths` / `&stale_paths` on the call line). The AST detector tracks
    // the alias: `raw`'s initializer reads `transition.stale_paths`.
    let evasion_alias = r#"
async fn sync_vue_alias_evasion(sync: ProjectSync) {
    let raw = transition.stale_paths;
    close_stale_provider_paths(&sync, &raw, "ctx").await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_alias = vue_functions_delegating_raw_stale_close_snippet(evasion_alias);
    assert!(
        flagged_alias.contains("sync_vue_alias_evasion"),
        "detector must FLAG an aliased `let raw = transition.stale_paths; \
         close_stale_provider_paths(&raw)` delegation, flagged={flagged_alias:?}"
    );

    // EVASION (alias via `.clone()`): a raw alias bound through `.clone()` of the
    // transition stale field is still a raw stale set.
    let evasion_alias_clone = r#"
async fn sync_vue_alias_clone_evasion(sync: ProjectSync) {
    let raw = transition.stale_paths.clone();
    close_provider_paths(&raw).await;
    let _ = sync.sync_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_alias_clone = vue_functions_delegating_raw_stale_close_snippet(evasion_alias_clone);
    assert!(
        flagged_alias_clone.contains("sync_vue_alias_clone_evasion"),
        "detector must FLAG an aliased `let raw = transition.stale_paths.clone(); \
         close_provider_paths(&raw)` delegation, flagged={flagged_alias_clone:?}"
    );

    // APPROVED (bare local, multiline call): the FILTERED `genuinely_stale` local
    // handed to the close helper across multiple lines must NOT flag — the local
    // is bound from `genuinely_stale_after_sync(..)`, so it is not a raw alias.
    let approved_multiline = r#"
async fn sync_vue_approved_multiline(sync: ProjectSync) {
    let _ = sync.open_tsx(&ide_path, ide_code).await;
    let stale_paths = transition.stale_paths;
    let genuinely_stale =
        genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
    close_provider_paths(
        &genuinely_stale,
    )
    .await;
}
"#;
    let flagged_approved_ml = vue_functions_delegating_raw_stale_close_snippet(approved_multiline);
    assert!(
        !flagged_approved_ml.contains("sync_vue_approved_multiline"),
        "detector must NOT flag a multiline close of the FILTERED `genuinely_stale` slice, \
         flagged={flagged_approved_ml:?}"
    );

    // NON-VUE (alias): a Shadow sync fn that aliases + closes the raw stale set —
    // it uses `*_file` (no `.vue` IDE/API primitive), so it is NOT `.vue`-syncing
    // and must NOT be flagged even with the alias delegation.
    let non_vue_alias = r#"
async fn sync_non_vue_alias(sync: ProjectSync) {
    let raw = transition.stale_paths;
    close_provider_paths(&raw).await;
    let _ = sync.open_file(&shadow_path, &code).await;
}
"#;
    let flagged_non_vue_alias = vue_functions_delegating_raw_stale_close_snippet(non_vue_alias);
    assert!(
        flagged_non_vue_alias.is_empty(),
        "detector must NOT flag a NON-Vue Shadow sync fn even with an aliased raw stale close, \
         flagged={flagged_non_vue_alias:?}"
    );

    // ── R6-2: wrapper/alias forms the prior LET-ALIAS detector still MISSED ──
    //
    // The R5-3 detector tracked taint through a `let` whose initializer reads a
    // `.stale_paths` FIELD, but it inspected only the TOP-LEVEL close-arg form
    // (`expr_is_raw_stale_field` / `expr_is_ref_to_local`) and did NOT propagate
    // taint through a local bound from ANOTHER tainted local. These four forms
    // each evaded it; the recursive-arg + fixed-point-taint detector flags them.

    // EVASION (direct wrapper `&...stale_paths.clone()`): the close-helper arg is
    // a reference to a `.clone()` METHOD CALL on the raw field — no `let` alias
    // at all. The prior `expr_is_raw_stale_field` unwrapped the `&` then saw a
    // MethodCall and returned false. The recursive arg scan sees the
    // `.stale_paths` field nested inside the arg expression tree.
    let evasion_wrapper_clone = r#"
async fn sync_vue_wrapper_clone_evasion(sync: ProjectSync) {
    close_provider_paths(&transition.stale_paths.clone()).await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_wrapper_clone =
        vue_functions_delegating_raw_stale_close_snippet(evasion_wrapper_clone);
    assert!(
        flagged_wrapper_clone.contains("sync_vue_wrapper_clone_evasion"),
        "detector must FLAG a direct-wrapper `close_provider_paths(&transition.stale_paths.clone())` \
         delegation in a `.vue` sync fn, flagged={flagged_wrapper_clone:?}"
    );

    // EVASION (by-value `.clone()` arg): the raw field is `.clone()`d and passed
    // BY VALUE (no `&`) directly as the close-helper argument. Same MethodCall
    // top-level shape the prior detector missed; the recursive arg scan finds
    // the `.stale_paths` field in the arg tree.
    let evasion_by_value_clone = r#"
async fn sync_vue_by_value_clone_evasion(sync: ProjectSync) {
    close_stale_paths(sync, transition.stale_paths.clone()).await;
    let _ = sync.sync_dts(&dts_path, &api.code).await;
}
"#;
    let flagged_by_value_clone =
        vue_functions_delegating_raw_stale_close_snippet(evasion_by_value_clone);
    assert!(
        flagged_by_value_clone.contains("sync_vue_by_value_clone_evasion"),
        "detector must FLAG a by-value `close_stale_paths(sync, transition.stale_paths.clone())` \
         delegation in a `.vue` sync fn, flagged={flagged_by_value_clone:?}"
    );

    // EVASION (multi-hop alias): taint flows raw field → local `a` → local `b`
    // (via `a.clone()`) → close-helper arg `&b`. The prior detector tainted `a`
    // (its initializer reads `.stale_paths`) but NOT `b` (`a.clone()` reads no
    // `.stale_paths` field), and the close arg `&b` referenced an untainted
    // local. The fixed-point taint walk propagates: `b`'s initializer reads the
    // tainted local `a`, so `b` is tainted too.
    let evasion_multi_hop = r#"
async fn sync_vue_multi_hop_alias_evasion(sync: ProjectSync) {
    let a = transition.stale_paths;
    let b = a.clone();
    close_provider_paths(&b).await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_multi_hop = vue_functions_delegating_raw_stale_close_snippet(evasion_multi_hop);
    assert!(
        flagged_multi_hop.contains("sync_vue_multi_hop_alias_evasion"),
        "detector must FLAG a multi-hop `let a = transition.stale_paths; let b = a.clone(); \
         close_provider_paths(&b)` delegation, flagged={flagged_multi_hop:?}"
    );

    // EVASION (Vec collected from a raw-stale LOCAL): the raw field is bound to
    // local `raw`, then a `Vec` is `.iter().cloned().collect()`ed FROM `raw`
    // (not directly from the field) into local `v`, and `&v` is closed. The
    // prior detector tainted `raw` but NOT `v` (`raw.iter().cloned().collect()`
    // reads no `.stale_paths` field). The fixed-point walk taints `v` because
    // its initializer reads the tainted local `raw`.
    let evasion_vec_collect = r#"
async fn sync_vue_vec_collect_evasion(sync: ProjectSync) {
    let raw = transition.stale_paths;
    let v: Vec<(ProviderPathKind, String)> = raw.iter().cloned().collect();
    close_stale_provider_paths(sync, &v, "ctx").await;
    let _ = sync.sync_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_vec_collect = vue_functions_delegating_raw_stale_close_snippet(evasion_vec_collect);
    assert!(
        flagged_vec_collect.contains("sync_vue_vec_collect_evasion"),
        "detector must FLAG a `Vec`-collected-from-a-raw-stale-local \
         `let v = raw.iter().cloned().collect(); close_stale_provider_paths(&v)` delegation, \
         flagged={flagged_vec_collect:?}"
    );

    // APPROVED (genuinely_stale wrapper): the FILTERED slice is `.clone()`d and
    // passed to the close helper. The `genuinely_stale` local is bound from
    // `genuinely_stale_after_sync(..)`, so it is not tainted; cloning a filtered
    // local stays filtered. Must NOT flag — proves the new recursive/fixed-point
    // detector still honours the filter exemption (does not flag every
    // `.clone()` close-arg).
    let approved_filtered_clone_wrapper = r#"
async fn sync_vue_approved_filtered_clone(sync: ProjectSync) {
    let _ = sync.open_tsx(&ide_path, ide_code).await;
    let stale_paths = transition.stale_paths;
    let genuinely_stale =
        genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
    close_provider_paths(&genuinely_stale.clone()).await;
}
"#;
    let flagged_approved_filtered_clone =
        vue_functions_delegating_raw_stale_close_snippet(approved_filtered_clone_wrapper);
    assert!(
        !flagged_approved_filtered_clone.contains("sync_vue_approved_filtered_clone"),
        "detector must NOT flag a close of the FILTERED `genuinely_stale.clone()` slice — the \
         recursive arg scan must honour the genuinely_stale_after_sync exemption, \
         flagged={flagged_approved_filtered_clone:?}"
    );

    // NON-VUE (multi-hop alias): a Shadow sync fn (uses `*_file`, NOT a `.vue`
    // IDE/API primitive) with the SAME multi-hop alias close — must NOT flag,
    // because it is outside the editor-liveness invariant. Pins that the new
    // detector keeps the `.vue`-syncing gate (it does not flag every multi-hop
    // raw-stale close, only `.vue`-syncing ones).
    let non_vue_multi_hop = r#"
async fn sync_non_vue_multi_hop(sync: ProjectSync) {
    let a = transition.stale_paths;
    let b = a.clone();
    close_provider_paths(&b).await;
    let _ = sync.open_file(&shadow_path, &code).await;
}
"#;
    let flagged_non_vue_multi_hop =
        vue_functions_delegating_raw_stale_close_snippet(non_vue_multi_hop);
    assert!(
        flagged_non_vue_multi_hop.is_empty(),
        "detector must NOT flag a NON-Vue Shadow sync fn even with a multi-hop aliased raw \
         stale close, flagged={flagged_non_vue_multi_hop:?}"
    );

    // ── Root-taint: PROJECTION/wrapper forms whose ROOT is a tainted local or a
    // raw `.stale_paths` field. The detector walks the close-arg expression tree
    // through references / parens / field-access / index / slice /
    // method-call-receiver to its root; if the root is a raw `.stale_paths` read
    // or a tainted local (and the subtree is NOT behind the
    // `genuinely_stale_after_sync(..)` filter boundary), the arg is tainted.
    // `let`-taint propagates the same way: a local bound from a rooted-tainted
    // initializer (including struct construction / aggregates that STORE a
    // tainted value) becomes tainted. These cases pin every projection form the
    // adversarial review enumerated. ──

    // FLAG (struct-field laundering, via intermediate tainted local): the raw set
    // is bound to `a`, stored into a struct field `Holder { paths: a }` bound to
    // `h`, and `&h.paths` is closed. Taint propagates raw field → `a` → (struct
    // construction stores a tainted value) → `h`; the close-arg `&h.paths` roots
    // in the tainted local `h`. Discriminates: a detector that only tracked
    // direct `.stale_paths`/alias close-args (not struct-field projection of a
    // tainted local) would MISS this.
    let evasion_struct_field = r#"
async fn sync_vue_struct_field_evasion(sync: ProjectSync) {
    let a = transition.stale_paths;
    let h = Holder { paths: a };
    close_provider_paths(&h.paths).await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_struct_field =
        vue_functions_delegating_raw_stale_close_snippet(evasion_struct_field);
    assert!(
        flagged_struct_field.contains("sync_vue_struct_field_evasion"),
        "detector must FLAG struct-field laundering `let h = Holder {{ paths: a }}; \
         close_provider_paths(&h.paths)` where `a` is a tainted local, flagged={flagged_struct_field:?}"
    );

    // FLAG (struct-field laundering, field initialised DIRECTLY from the raw
    // field): `let h = Holder { paths: transition.stale_paths };` then
    // `&h.paths`. No intermediate local — the struct initializer itself reads the
    // raw `.stale_paths` field, so `h` is tainted at construction. Discriminates
    // the aggregate-construction propagation path with no alias hop.
    let evasion_struct_field_direct = r#"
async fn sync_vue_struct_field_direct_evasion(sync: ProjectSync) {
    let h = Holder { paths: transition.stale_paths };
    close_provider_paths(&h.paths).await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_struct_field_direct =
        vue_functions_delegating_raw_stale_close_snippet(evasion_struct_field_direct);
    assert!(
        flagged_struct_field_direct.contains("sync_vue_struct_field_direct_evasion"),
        "detector must FLAG struct construction reading the raw field directly \
         `let h = Holder {{ paths: transition.stale_paths }}; close_provider_paths(&h.paths)`, \
         flagged={flagged_struct_field_direct:?}"
    );

    // FLAG (index/slice projection rooted in a tainted local): `&raw[..]` where
    // `raw` is the raw stale set. The close-arg is a reference to an index/slice
    // expression whose base roots in the tainted local. Discriminates: a detector
    // that unwrapped only `&local` (not `&local[..]`) would MISS the slice.
    let evasion_index_slice = r#"
async fn sync_vue_index_slice_evasion(sync: ProjectSync) {
    let raw = transition.stale_paths;
    close_provider_paths(&raw[..]).await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_index_slice = vue_functions_delegating_raw_stale_close_snippet(evasion_index_slice);
    assert!(
        flagged_index_slice.contains("sync_vue_index_slice_evasion"),
        "detector must FLAG an index/slice `close_provider_paths(&raw[..])` rooted in a tainted \
         local, flagged={flagged_index_slice:?}"
    );

    // FLAG (`.as_slice()` method-receiver projection rooted in a tainted local):
    // `raw.as_slice()` passed by value. The close-arg is a method call whose
    // receiver roots in the tainted local. Discriminates the method-receiver
    // projection path (a detector unwrapping only references/fields would MISS a
    // method-call projection of the tainted set).
    let evasion_as_slice = r#"
async fn sync_vue_as_slice_evasion(sync: ProjectSync) {
    let raw = transition.stale_paths;
    close_provider_paths(raw.as_slice()).await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_as_slice = vue_functions_delegating_raw_stale_close_snippet(evasion_as_slice);
    assert!(
        flagged_as_slice.contains("sync_vue_as_slice_evasion"),
        "detector must FLAG a method-receiver projection `close_provider_paths(raw.as_slice())` \
         rooted in a tainted local, flagged={flagged_as_slice:?}"
    );

    // FLAG (method-receiver projection rooted in a tainted local, behind a `&`):
    // `&raw.to_owned()`. The close-arg references a method call whose receiver is
    // the tainted local. Pins that a `&`-wrapped method-receiver projection is
    // walked to its root.
    let evasion_method_receiver = r#"
async fn sync_vue_method_receiver_evasion(sync: ProjectSync) {
    let raw = transition.stale_paths;
    close_provider_paths(&raw.to_owned()).await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_method_receiver =
        vue_functions_delegating_raw_stale_close_snippet(evasion_method_receiver);
    assert!(
        flagged_method_receiver.contains("sync_vue_method_receiver_evasion"),
        "detector must FLAG a `&`-wrapped method-receiver projection \
         `close_provider_paths(&raw.to_owned())` rooted in a tainted local, \
         flagged={flagged_method_receiver:?}"
    );

    // FLAG (in-function closure capture): `let f = || close_provider_paths(&raw); f().await;`.
    // The close call is lexically INSIDE the `.vue`-syncing function (in the
    // closure body), so the within-fn syn walk reaches it and roots the close-arg
    // `&raw` in the tainted local. This is the explicit closure-capture DECISION:
    // the common in-fn `let f = || close(&raw); f().await;` shape IS covered.
    // (Closures that ESCAPE — moved/returned out of the fn and called elsewhere —
    // are a named residual below.) Discriminates the closure-body reach.
    let evasion_closure_capture = r#"
async fn sync_vue_closure_capture_evasion(sync: ProjectSync) {
    let raw = transition.stale_paths;
    let f = || close_provider_paths(&raw);
    f().await;
    let _ = sync.open_tsx(&ide_path, ide_code).await;
}
"#;
    let flagged_closure_capture =
        vue_functions_delegating_raw_stale_close_snippet(evasion_closure_capture);
    assert!(
        flagged_closure_capture.contains("sync_vue_closure_capture_evasion"),
        "detector must FLAG an in-function closure-capture `let f = || close_provider_paths(&raw); \
         f().await` whose body closes a tainted local, flagged={flagged_closure_capture:?}"
    );

    // NEGATIVE (genuinely_stale result PROJECTED): the FILTERED `genuinely_stale`
    // local is index-sliced (`&genuinely_stale[..]`) into the close helper. The
    // local is bound from `genuinely_stale_after_sync(..)`, so it is not tainted;
    // projecting a filtered local stays filtered. Must NOT flag — proves the
    // root-taint walk honours the filter boundary even under a projection wrapper.
    let negative_genuinely_projected = r#"
async fn sync_vue_genuinely_projected(sync: ProjectSync) {
    let _ = sync.open_tsx(&ide_path, ide_code).await;
    let stale_paths = transition.stale_paths;
    let genuinely_stale = genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
    close_provider_paths(&genuinely_stale[..]).await;
}
"#;
    let flagged_genuinely_projected =
        vue_functions_delegating_raw_stale_close_snippet(negative_genuinely_projected);
    assert!(
        !flagged_genuinely_projected.contains("sync_vue_genuinely_projected"),
        "detector must NOT flag a PROJECTED `genuinely_stale` slice `&genuinely_stale[..]` — the \
         root-taint walk must keep the genuinely_stale_after_sync exemption under projection, \
         flagged={flagged_genuinely_projected:?}"
    );

    // NEGATIVE (genuinely_stale result projected via method-receiver): the
    // FILTERED local is passed via `genuinely_stale.as_slice()`. Same filter
    // boundary under a method-receiver projection — must NOT flag.
    let negative_genuinely_as_slice = r#"
async fn sync_vue_genuinely_as_slice(sync: ProjectSync) {
    let _ = sync.open_tsx(&ide_path, ide_code).await;
    let stale_paths = transition.stale_paths;
    let genuinely_stale = genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
    close_provider_paths(genuinely_stale.as_slice()).await;
}
"#;
    let flagged_genuinely_as_slice =
        vue_functions_delegating_raw_stale_close_snippet(negative_genuinely_as_slice);
    assert!(
        !flagged_genuinely_as_slice.contains("sync_vue_genuinely_as_slice"),
        "detector must NOT flag a method-receiver projection of the FILTERED `genuinely_stale` \
         slice `genuinely_stale.as_slice()`, flagged={flagged_genuinely_as_slice:?}"
    );

    // NEGATIVE (non-Vue Shadow close of a struct-laundered raw set): the SAME
    // struct-field laundering as the FLAG case above, but in a Shadow sync fn
    // (uses `*_file`, NOT a `.vue` IDE/API primitive). It is outside the
    // editor-liveness invariant, so it must NOT flag. Pins that the root-taint
    // walk keeps the `.vue`-syncing gate (it does not flag every projected
    // raw-stale close, only `.vue`-syncing ones).
    let negative_non_vue_struct = r#"
async fn sync_non_vue_struct_field(sync: ProjectSync) {
    let a = transition.stale_paths;
    let h = Holder { paths: a };
    close_provider_paths(&h.paths).await;
    let _ = sync.open_file(&shadow_path, &code).await;
}
"#;
    let flagged_non_vue_struct =
        vue_functions_delegating_raw_stale_close_snippet(negative_non_vue_struct);
    assert!(
        flagged_non_vue_struct.is_empty(),
        "detector must NOT flag a NON-Vue Shadow sync fn even with struct-field-laundered raw \
         stale close, flagged={flagged_non_vue_struct:?}"
    );

    // ── HONEST BOUNDARY STATEMENT (replaces the prior absolute over-claim) ──
    //
    // What the within-function `syn` walk DOES detect (each pinned by a
    // discriminating FLAG case above or in the R5-3 / R6-2 blocks): a close-helper
    // argument whose expression tree — walked through references, parens/groups,
    // named field access, index/slice, and method-call receivers — roots in a raw
    // `.stale_paths` field read or a `let`-tainted local, where the taint of a
    // local propagates (to a fixed point) through direct field reads, `.clone()` /
    // by-value wrappers, `.iter().cloned().collect()` / `.to_vec()`, single- and
    // multi-hop alias chains, struct construction / aggregates that STORE a
    // tainted value, and arbitrarily-nested projection subexpressions; the
    // `genuinely_stale_after_sync(..)` call is a filter boundary the walk never
    // descends into, and only `.vue`-syncing functions are in scope. The common
    // in-function closure-capture shape (`let f = || close(&raw); f().await;`) is
    // covered because the close call is lexically inside the function body the
    // walk visits (pinned by `sync_vue_closure_capture_evasion`).
    //
    // What it CANNOT detect — the NAMED residuals (each crosses a syntax / module
    // boundary a within-function `syn` walk does not expand, so NONE of them is
    // claimed as covered):
    //
    //   1. MACRO-generated close calls — a stale set laundered through a macro
    //      expansion (`close_provider_paths(stale_macro!(transition))`); the macro
    //      body is not expanded by the guard's token walk.
    //   2. CROSS-MODULE / OPAQUE-HELPER laundering — the raw set passed through an
    //      opaque helper whose body lives in another fn/module
    //      (`close_provider_paths(launder(transition))`); `launder` is not the
    //      `genuinely_stale_after_sync` filter, but the guard cannot see that its
    //      body returns raw stale, so it does not flag.
    //   3. ESCAPING-CLOSURE laundering — taint captured into a closure that is
    //      MOVED or RETURNED out of the function and invoked elsewhere; the close
    //      call site is then in a different function than the capture.
    //   4. RETURNED-AGGREGATE laundering — taint stored into a struct field that
    //      is RETURNED and closed by a DIFFERENT function; no close-helper call
    //      exists in the producing function for the guard to attribute.
    //
    // The behavioral safety net for ALL FOUR residuals is the runtime
    // close-AFTER-successful-sync integration tests — they exercise the actual
    // close ordering regardless of how the stale set reached the close call:
    // `crate::sync_coordinator`'s
    // `sync_file_retains_stale_paths_when_owner_change_sync_fails` and
    // `crate::workspace_scanner`'s
    // `scanner_sync_retains_stale_paths_when_owner_change_sync_fails` assert the
    // prior path is NOT closed when the replacement sync fails. The presence of
    // those runtime guards is asserted here so this static guard never silently
    // becomes the sole safety net for the residuals it cannot cover.
    assert_runtime_close_after_sync_safety_net_present();
}

/// Asserts the behavioral safety net for the static detector's NAMED residuals
/// (macro-generated / cross-module-opaque / escaping-closure / returned-aggregate
/// laundering) is present: the runtime close-AFTER-successful-sync integration
/// tests that exercise the actual close ordering regardless of how the stale set
/// reached the close call. Reads the owning source files and asserts each
/// regression test still exists, so a refactor that deletes the behavioral net
/// (leaving only this static guard, which cannot see the residual forms) FAILS
/// here loudly.
fn assert_runtime_close_after_sync_safety_net_present() {
    let cases: &[(&str, &str)] = &[
        (
            "src/sync_coordinator.rs",
            "sync_file_retains_stale_paths_when_owner_change_sync_fails",
        ),
        (
            "src/workspace_scanner.rs",
            "scanner_sync_retains_stale_paths_when_owner_change_sync_fails",
        ),
    ];
    for (rel, test_name) in cases {
        let path = crate_src_path(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("safety-net guard must read {}: {e}", path.display()));
        assert!(
            src.contains(&format!("fn {test_name}")),
            "runtime close-after-sync safety net missing: `{rel}` must keep the regression test \
             `fn {test_name}` — it is the behavioral net for the static detector's residuals \
             (macro / cross-module-opaque / escaping-closure / returned-aggregate laundering), \
             which a within-fn syn walk cannot attribute."
        );
    }
}
