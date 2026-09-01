//! Host-backend compile-routing architecture guards.
//!
//! These pin the routing of the session's runtime compile: the host-backed
//! compile in `compile_entry` consumes the request-scoped bound host request
//! and executes through the registered `FrameworkHostIntegrationBackend`
//! (demand-specific admission consumed by value by the product execution) —
//! never a hardcoded framework producer and never a combined-registry
//! `compile_bundle` dispatch. The IDE-ensure path is an explicit demand
//! enum — `ensure_ide_compiled` never requests `VirtualNodeKind::Main` and
//! `get_ide` never computes on read.
//!
//! The no-hardcode guard is an AST/`syn` scan (NOT a substring scan): it
//! parses the two files that carry the host-backed route, walks
//! `compile_entry`'s / `execute_bound_host_products`'s bodies plus the
//! one-level local helpers they reach, and asserts none calls a hardcoded
//! Vue free function (`compile` / `compile_from_parsed` / `compile_sfc` /
//! `vue_parse`) or a combined-registry bundle route
//! (`compile_bundle` / `compiler_for_carrier_language`); it also inspects
//! each file's `use` declarations for any alias, rename, or glob that would
//! re-bind one of those symbols under a new name.
//!
//! The positive half is asserted PER DISPATCH ARM: every arm of the bound
//! host request's dispatch must reach the backend's issuance and its
//! by-value execution. A whole-body requirement would be framework-blind —
//! one arm rewired to bypass admission still reads as satisfied through its
//! sibling's calls — and a newly added framework arm is covered
//! automatically.
//!
//! Negative self-tests prove the guard catches a renamed import, a
//! method-hidden producer, a reintroduced registry-bundle call, and a
//! single-arm admission bypass.

use std::fs;
use std::path::PathBuf;

use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, File, Item, ItemFn, UseTree};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_workspace_file(rel: &str) -> String {
    fs::read_to_string(workspace_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

const PIPELINE_REL: &str = "crates/verter_session/src/host_resolve/virtual_file_pipeline.rs";
const BOUND_EXECUTION_REL: &str = "crates/verter_session/src/host_resolve/compile_request_build.rs";

/// The hardcoded Vue runtime producers the host-backed route must NOT call.
const FORBIDDEN_PRODUCERS: [&str; 4] =
    ["compile", "compile_from_parsed", "compile_sfc", "vue_parse"];

/// The combined-registry bundle route the host-backed route must NOT
/// dispatch through: execution goes through the bound framework
/// host-integration backend's admission, never an outer registry-selected
/// bundle call.
const FORBIDDEN_REGISTRY_ROUTE: [&str; 2] = ["compile_bundle", "compiler_for_carrier_language"];

/// The registered-host-backend consumption seam the bound execution MUST
/// call: demand-specific issuance plus the by-value product execution.
///
/// Required PER DISPATCH ARM, never merely somewhere in the function: a
/// whole-body requirement is framework-blind, because one arm rewired to
/// bypass admission still reads as satisfied through its sibling's calls.
const REQUIRED_BOUND_CALLS: [&str; 2] = ["admit_host_products", "compile_host_products"];

/// The sealed sum the bound execution dispatches on — one arm per bound
/// framework host request.
const BOUND_DISPATCH_ENUM: &str = "BoundNativeHostRequest";

/// Collects every free / associated CALL name and every method-CALL name in a
/// body so we can assert a forbidden producer is never invoked.
struct CallNameCollector {
    free_calls: Vec<String>,
    method_calls: Vec<String>,
}

impl<'ast> Visit<'ast> for CallNameCollector {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(p) = call.func.as_ref() {
            if let Some(last) = p.path.segments.last() {
                self.free_calls.push(last.ident.to_string());
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
    fn visit_expr_method_call(&mut self, mc: &'ast ExprMethodCall) {
        self.method_calls.push(mc.method.to_string());
        syn::visit::visit_expr_method_call(self, mc);
    }
}

/// Index every free `fn` in a parsed file by name (so an anchor body's
/// one-level local callees can be inspected — an indirection through a local
/// helper must not hide a forbidden producer call).
fn index_free_fns(file: &File) -> std::collections::HashMap<String, ItemFn> {
    let mut out = std::collections::HashMap::new();
    fn walk(items: &[Item], out: &mut std::collections::HashMap<String, ItemFn>) {
        for item in items {
            match item {
                Item::Fn(f) => {
                    out.insert(f.sig.ident.to_string(), f.clone());
                }
                Item::Impl(i) => {
                    for impl_item in &i.items {
                        if let syn::ImplItem::Fn(m) = impl_item {
                            // Methods are keyed by name too — `compile_entry`
                            // is an impl method; a forbidden call inside a
                            // sibling method it calls is still in scope.
                            out.insert(
                                m.sig.ident.to_string(),
                                ItemFn {
                                    attrs: m.attrs.clone(),
                                    vis: syn::Visibility::Inherited,
                                    sig: m.sig.clone(),
                                    block: Box::new(m.block.clone()),
                                },
                            );
                        }
                    }
                }
                Item::Mod(m) => {
                    if let Some((_, inner)) = &m.content {
                        walk(inner, out);
                    }
                }
                _ => {}
            }
        }
    }
    walk(&file.items, &mut out);
    out
}

/// A `use` binding the guard cares about: a named/aliased import (the bound
/// identifiers) or a glob whose parent path crosses the compiler `compile`
/// module (which exports the forbidden producers — a glob there could re-bind
/// `compile_sfc` / `compile_from_parsed` / `compile` invisibly).
#[derive(Default)]
struct UseBindings {
    /// Identifiers any named / aliased import binds.
    idents: Vec<String>,
    /// `true` when a glob crosses `verter_compiler::compile` (or `crate::compile`),
    /// the only globs that could import a forbidden producer.
    compiler_compile_glob: bool,
}

/// Walk a `use` tree, recording bound identifiers and whether a glob crosses
/// the compiler `compile` module. `path` is the accumulated path-segment stack.
fn collect_use_tree(tree: &UseTree, path: &mut Vec<String>, out: &mut UseBindings) {
    match tree {
        UseTree::Path(p) => {
            path.push(p.ident.to_string());
            collect_use_tree(&p.tree, path, out);
            path.pop();
        }
        UseTree::Name(n) => out.idents.push(n.ident.to_string()),
        UseTree::Rename(r) => {
            // Both the original and the alias matter: an alias of a forbidden
            // producer (`use ...::compile_sfc as cmp`) re-binds it.
            out.idents.push(r.ident.to_string());
            out.idents.push(r.rename.to_string());
        }
        UseTree::Glob(_) => {
            // Only a glob crossing the compiler `compile` module can re-bind a
            // forbidden producer. A glob over an unrelated module
            // (`crate::types::*`) cannot — it is not a violation.
            let crosses_compiler_compile = path
                .windows(2)
                .any(|w| w[0] == "verter_compiler" && w[1] == "compile")
                || path.last().map(|s| s == "compile").unwrap_or(false);
            if crosses_compiler_compile {
                out.compiler_compile_glob = true;
            }
        }
        UseTree::Group(g) => {
            for t in &g.items {
                collect_use_tree(t, path, out);
            }
        }
    }
}

/// Every `use` binding in the file (named idents + a compiler-`compile`-glob flag).
fn all_use_bindings(file: &File) -> UseBindings {
    let mut out = UseBindings::default();
    fn walk(items: &[Item], out: &mut UseBindings) {
        for item in items {
            match item {
                Item::Use(u) => {
                    let mut path = Vec::new();
                    collect_use_tree(&u.tree, &mut path, out);
                }
                Item::Mod(m) => {
                    if let Some((_, inner)) = &m.content {
                        walk(inner, out);
                    }
                }
                _ => {}
            }
        }
    }
    walk(&file.items, &mut out);
    out
}

/// Every call name reachable from the anchor's body plus its one-level local
/// callees (free fns AND same-impl methods, both followed one level).
fn calls_reachable_from(file: &File, anchor: &str) -> Vec<String> {
    let fns = index_free_fns(file);
    let entry = fns
        .get(anchor)
        .unwrap_or_else(|| panic!("{anchor} not found — guard anchor moved"));

    let mut collector = CallNameCollector {
        free_calls: Vec::new(),
        method_calls: Vec::new(),
    };
    collector.visit_block(&entry.block);
    expand_one_level(&fns, collector)
}

/// Given the calls collected from some body, add the calls of every local
/// free fn / same-impl method it reaches, one level deep.
fn expand_one_level(
    fns: &std::collections::HashMap<String, ItemFn>,
    collector: CallNameCollector,
) -> Vec<String> {
    // One-level local helper expansion: any free fn OR same-impl method the
    // anchor calls is also scanned. `index_free_fns` keys BOTH free functions
    // and impl methods by name, so a free-call indirection
    // (`fn h() { compile_sfc(...) }`) AND a method-call indirection
    // (`self.helper()` where `fn helper(&self) { compile_sfc(...) }`) are both
    // followed one level. A body-only scan that ignored `self.method()` would
    // let a future anchor hide a forbidden producer call behind a sibling
    // method — this expansion closes that.
    let mut seed: Vec<String> = collector.free_calls.clone();
    seed.extend(collector.method_calls.clone());
    let mut helper_calls: Vec<String> = Vec::new();
    for callee in &seed {
        if let Some(f) = fns.get(callee) {
            let mut hc = CallNameCollector {
                free_calls: Vec::new(),
                method_calls: Vec::new(),
            };
            hc.visit_block(&f.block);
            // Both the free CALLS and the method CALLS inside the reached
            // helper count — the helper could itself call the producer as a
            // free fn (`compile_sfc(...)`) or as a method (`self.compile(...)`).
            helper_calls.extend(hc.free_calls);
            helper_calls.extend(hc.method_calls);
        }
    }

    let mut all_calls: Vec<String> = collector.free_calls;
    all_calls.extend(collector.method_calls);
    all_calls.extend(helper_calls);
    all_calls
}

/// The variant an arm pattern binds, when the pattern names `dispatch_enum`
/// as the owning path segment. `BoundNativeHostRequest::Vue(bound)` yields
/// `Some("Vue")`; an unrelated `match` in the same body yields `None`, so a
/// sibling match cannot be mistaken for the framework dispatch.
fn dispatch_variant_of_pattern(pat: &syn::Pat, dispatch_enum: &str) -> Option<String> {
    let path = match pat {
        syn::Pat::TupleStruct(t) => &t.path,
        syn::Pat::Path(p) => &p.path,
        syn::Pat::Struct(s) => &s.path,
        _ => return None,
    };
    let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let variant = segments.last()?;
    let owner = segments.get(segments.len().checked_sub(2)?)?;
    (owner == dispatch_enum).then(|| variant.clone())
}

/// Collects the bodies of every `match` arm that dispatches on the bound
/// framework host request, keyed by the bound variant.
struct DispatchArmCollector<'a> {
    dispatch_enum: &'a str,
    arms: Vec<(String, Expr)>,
}

impl<'ast> Visit<'ast> for DispatchArmCollector<'_> {
    fn visit_expr_match(&mut self, m: &'ast syn::ExprMatch) {
        for arm in &m.arms {
            if let Some(variant) = dispatch_variant_of_pattern(&arm.pat, self.dispatch_enum) {
                self.arms.push((variant, (*arm.body).clone()));
            }
        }
        syn::visit::visit_expr_match(self, m);
    }
}

/// Every framework dispatch arm reachable in `anchor`, mapped to the calls
/// that arm makes (its own body plus its one-level local callees).
///
/// PER-ARM, deliberately: a positive routing requirement checked over the
/// whole function body is arm-blind — rewiring one framework arm to bypass
/// the admission seam would still satisfy it through the sibling arm's
/// calls.
fn dispatch_arm_calls(
    file: &File,
    anchor: &str,
    dispatch_enum: &str,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let fns = index_free_fns(file);
    let entry = fns
        .get(anchor)
        .unwrap_or_else(|| panic!("{anchor} not found — guard anchor moved"));

    let mut arms = DispatchArmCollector {
        dispatch_enum,
        arms: Vec::new(),
    };
    arms.visit_block(&entry.block);

    let mut out = std::collections::BTreeMap::new();
    for (variant, body) in arms.arms {
        let mut collector = CallNameCollector {
            free_calls: Vec::new(),
            method_calls: Vec::new(),
        };
        collector.visit_expr(&body);
        out.entry(variant)
            .or_insert_with(Vec::new)
            .extend(expand_one_level(&fns, collector));
    }
    out
}

/// Walk an anchor's body + its one-level local callees and return the set of
/// forbidden calls invoked. Shared by the live guard and the negative
/// self-tests (which run it over synthetic files).
fn forbidden_calls_reachable_from(file: &File, anchor: &str, forbidden: &[&str]) -> Vec<String> {
    calls_reachable_from(file, anchor)
        .into_iter()
        .filter(|c| forbidden.contains(&c.as_str()))
        .collect()
}

/// THE routing guard. The host-backed compile routes through the registered
/// framework host-integration backend: `compile_entry` (and the bound
/// execution it dispatches, `execute_bound_host_products`) must NOT call a
/// hardcoded Vue producer and must NOT dispatch a combined-registry bundle
/// route, neither file may import a forbidden producer under any alias /
/// rename / glob, and the bound execution MUST reach the registered
/// backend's demand-specific issuance + by-value product execution.
#[test]
fn compile_entry_routes_through_registered_host_backend_not_hardcoded_vue() {
    let pipeline = syn::parse_file(&read_workspace_file(PIPELINE_REL))
        .expect("parse virtual_file_pipeline.rs");
    let bound = syn::parse_file(&read_workspace_file(BOUND_EXECUTION_REL))
        .expect("parse compile_request_build.rs");

    // (1) No forbidden producer and no registry bundle route is CALLED in
    // `compile_entry` / `execute_bound_host_products` (or their local
    // helpers).
    let forbidden: Vec<&str> = FORBIDDEN_PRODUCERS
        .iter()
        .chain(FORBIDDEN_REGISTRY_ROUTE.iter())
        .copied()
        .collect();
    for (file, anchor) in [
        (&pipeline, "compile_entry"),
        (&bound, "execute_bound_host_products"),
    ] {
        let bad_calls = forbidden_calls_reachable_from(file, anchor, &forbidden);
        assert!(
            bad_calls.is_empty(),
            "host-backend routing: `{anchor}` calls the forbidden producer/route(s) \
             {bad_calls:?}. The host-backed compile MUST consume the request-scoped bound \
             host request and execute through the registered framework host-integration \
             backend (`admit_host_products` -> `compile_host_products`) — delete the direct \
             `compile` / `compile_from_parsed` / `compile_sfc` / `vue_parse` use and any \
             registry `compile_bundle` dispatch."
        );
    }

    // (2) Neither file may IMPORT a forbidden producer under any name. A glob
    // import crossing the compiler `compile` module (`use
    // verter_compiler::compile::*`) could re-bind `compile_sfc` /
    // `compile_from_parsed` / `compile` into scope without a named `use`, so
    // such a glob is itself a violation here (both files import their
    // compiler symbols explicitly). An unrelated glob (`crate::types::*`) is
    // fine.
    for (file, rel) in [(&pipeline, PIPELINE_REL), (&bound, BOUND_EXECUTION_REL)] {
        let bindings = all_use_bindings(file);
        let imported_forbidden: Vec<&String> = bindings
            .idents
            .iter()
            .filter(|b| FORBIDDEN_PRODUCERS.contains(&b.as_str()))
            .collect();
        assert!(
            imported_forbidden.is_empty(),
            "host-backend routing: `{rel}` imports the hardcoded Vue producer(s) \
             {imported_forbidden:?} — even an alias / rename re-binds them. The runtime \
             producer is reached through the registered host backend, not a direct import."
        );
        assert!(
            !bindings.compiler_compile_glob,
            "host-backend routing: `{rel}` has a glob `use` crossing `verter_compiler::compile` \
             — a glob there could re-bind a hardcoded Vue producer (`compile_sfc`, …) into \
             scope invisibly. Import compiler symbols explicitly so this guard can verify the \
             forbidden producers are absent."
        );
    }

    // (3) POSITIVE selection evidence, PER FRAMEWORK ARM: every arm of the
    // bound-request dispatch consumes the registered host backend through
    // its demand-specific issuance AND its by-value product execution. A
    // whole-body check would be arm-blind — one arm rewired to bypass
    // admission would still be covered by its sibling's calls — so the
    // requirement is asserted separately for each arm the dispatch carries,
    // and a newly added framework arm is covered automatically.
    let arms = dispatch_arm_calls(&bound, "execute_bound_host_products", BOUND_DISPATCH_ENUM);
    assert!(
        arms.len() >= 2,
        "host-backend routing: found {} `{BOUND_DISPATCH_ENUM}` dispatch arm(s) in \
         `execute_bound_host_products` (expected the registered framework arms). The bound \
         execution must dispatch on the sealed bound-request sum; if the dispatch moved, \
         retarget this guard rather than dropping the per-arm requirement. Found: {:?}",
        arms.len(),
        arms.keys().collect::<Vec<_>>()
    );
    for (variant, arm_calls) in &arms {
        for required in REQUIRED_BOUND_CALLS {
            assert!(
                arm_calls.iter().any(|c| c == required),
                "host-backend routing: the `{BOUND_DISPATCH_ENUM}::{variant}` arm of \
                 `execute_bound_host_products` does not reach `{required}` — EVERY framework \
                 arm must issue the demand-specific admission on its registered backend and \
                 execute it by value, not just one of them. Arm calls: {arm_calls:?}"
            );
        }
    }
}

/// NEGATIVE self-test: the guard MUST catch a renamed import + aliased call of
/// a forbidden producer. This proves the guard discriminates (it fails against
/// a violating tree) — without it the positive test could pass vacuously.
#[test]
fn no_hardcode_guard_catches_a_renamed_producer_import_and_call() {
    // A synthetic pipeline file that RENAMES the forbidden
    // `compile_from_parsed` producer to `cfp` and calls it inside an impl
    // method named `compile_entry`. A whole-word scan for `compile_from_parsed`
    // would miss the renamed CALL; the AST guard catches it via the `use`
    // alias inspection.
    let violating = r#"
        use verter_compiler::compile::compile_from_parsed as cfp;
        struct H;
        impl H {
            fn compile_entry(&self) -> u32 {
                let _ = cfp();
                0
            }
        }
    "#;
    let file = syn::parse_file(violating).expect("parse synthetic violating file");

    // The alias inspection catches the renamed import (`compile_from_parsed`
    // appears as the original ident of the `as` rename).
    let bindings = all_use_bindings(&file);
    let caught_import = bindings
        .idents
        .iter()
        .any(|b| FORBIDDEN_PRODUCERS.contains(&b.as_str()));
    assert!(
        caught_import,
        "the no-hardcode guard FAILED to catch a renamed forbidden-producer import — \
         the guard does not discriminate"
    );

    // And to be thorough: a DIRECT (un-aliased) call is caught by the call scan.
    let direct = r#"
        struct H;
        impl H {
            fn compile_entry(&self) -> u32 {
                let _ = compile_sfc();
                0
            }
        }
    "#;
    let direct_file = syn::parse_file(direct).expect("parse synthetic direct-call file");
    let bad = forbidden_calls_reachable_from(&direct_file, "compile_entry", &FORBIDDEN_PRODUCERS);
    assert!(
        bad.contains(&"compile_sfc".to_string()),
        "the no-hardcode guard FAILED to catch a direct `compile_sfc()` call in compile_entry"
    );

    // A glob crossing `verter_compiler::compile` is caught …
    let compiler_glob =
        syn::parse_file("use verter_compiler::compile::*;").expect("parse compiler-compile glob");
    assert!(
        all_use_bindings(&compiler_glob).compiler_compile_glob,
        "the guard FAILED to flag a `verter_compiler::compile::*` glob"
    );
    // … while an UNRELATED glob is NOT flagged (discrimination: the real file
    // legitimately has `use crate::types::*;`).
    let unrelated_glob = syn::parse_file("use crate::types::*;").expect("parse unrelated glob");
    assert!(
        !all_use_bindings(&unrelated_glob).compiler_compile_glob,
        "the guard wrongly flagged an unrelated `crate::types::*` glob — it does not discriminate"
    );
}

/// NEGATIVE self-test: a reintroduced combined-registry bundle dispatch
/// (`compiler.compile_bundle(...)` behind a registry lookup) inside
/// `compile_entry` is caught by the registry-route scan — and a clean tree
/// calling only the bound-backend seams is NOT flagged.
#[test]
fn routing_guard_catches_a_reintroduced_registry_bundle_dispatch() {
    let forbidden: Vec<&str> = FORBIDDEN_PRODUCERS
        .iter()
        .chain(FORBIDDEN_REGISTRY_ROUTE.iter())
        .copied()
        .collect();

    let violating = r#"
        struct H;
        impl H {
            fn compile_entry(&self) -> u32 {
                let compiler = carrier_compiler_registry()
                    .compiler_for_carrier_language();
                compiler.compile_bundle()
            }
        }
    "#;
    let file = syn::parse_file(violating).expect("parse synthetic registry-dispatch file");
    let bad = forbidden_calls_reachable_from(&file, "compile_entry", &forbidden);
    assert!(
        bad.contains(&"compile_bundle".to_string())
            && bad.contains(&"compiler_for_carrier_language".to_string()),
        "the routing guard FAILED to catch a reintroduced registry `compile_bundle` \
         dispatch in compile_entry. Got: {bad:?}"
    );

    // Discrimination floor: the bound-backend seams themselves must NOT be
    // flagged.
    let clean = r#"
        struct H;
        impl H {
            fn compile_entry(&self) -> u32 {
                let admission = backend.admit_host_products();
                backend.compile_host_products(admission)
            }
        }
    "#;
    let clean_file = syn::parse_file(clean).expect("parse clean bound-backend file");
    let clean_calls = forbidden_calls_reachable_from(&clean_file, "compile_entry", &forbidden);
    assert!(
        clean_calls.is_empty(),
        "the routing guard wrongly flagged the bound-backend consumption seams — it does \
         not discriminate. Got: {clean_calls:?}"
    );
}

/// NEGATIVE self-test: the per-arm positive requirement MUST catch a
/// SINGLE-ARM bypass. A dispatch whose Vue arm issues and executes the
/// admission while its Svelte arm calls a producer directly satisfies any
/// whole-body "the seams appear somewhere" check — the Vue arm's calls cover
/// for the Svelte arm. The per-arm check flags exactly the bypassing arm.
#[test]
fn routing_guard_catches_a_single_arm_admission_bypass() {
    let single_arm_bypass = r#"
        fn execute_bound_host_products(binding: B) -> u32 {
            match binding {
                BoundNativeHostRequest::Vue(bound) => {
                    let admission = backend.admit_host_products(demand);
                    backend.compile_host_products(admission)
                }
                BoundNativeHostRequest::Svelte(bound) => bundle_it(bound),
            }
        }
    "#;
    let file = syn::parse_file(single_arm_bypass).expect("parse single-arm-bypass file");
    let arms = dispatch_arm_calls(&file, "execute_bound_host_products", BOUND_DISPATCH_ENUM);
    assert_eq!(
        arms.keys().cloned().collect::<Vec<_>>(),
        vec!["Svelte".to_string(), "Vue".to_string()],
        "the arm collector must find BOTH framework arms of the dispatch"
    );
    let bypassing: Vec<&String> = arms
        .iter()
        .filter(|(_, calls)| {
            !REQUIRED_BOUND_CALLS
                .iter()
                .all(|required| calls.iter().any(|c| c == required))
        })
        .map(|(variant, _)| variant)
        .collect();
    assert_eq!(
        bypassing,
        vec!["Svelte"],
        "the per-arm requirement FAILED to isolate the single bypassing arm — a whole-body \
         check would read this tree as compliant because the Vue arm calls both seams"
    );

    // Discrimination floor: when BOTH arms reach both seams, no arm is
    // flagged — the per-arm check is not simply always-failing.
    let compliant = r#"
        fn execute_bound_host_products(binding: B) -> u32 {
            match binding {
                BoundNativeHostRequest::Vue(bound) => {
                    let admission = backend.admit_host_products(demand);
                    backend.compile_host_products(admission)
                }
                BoundNativeHostRequest::Svelte(bound) => {
                    let admission = backend.admit_host_products(demand);
                    backend.compile_host_products(admission)
                }
            }
        }
    "#;
    let compliant_file = syn::parse_file(compliant).expect("parse compliant two-arm file");
    let compliant_arms = dispatch_arm_calls(
        &compliant_file,
        "execute_bound_host_products",
        BOUND_DISPATCH_ENUM,
    );
    assert_eq!(compliant_arms.len(), 2);
    for (variant, calls) in &compliant_arms {
        for required in REQUIRED_BOUND_CALLS {
            assert!(
                calls.iter().any(|c| c == required),
                "the per-arm check wrongly flagged the compliant `{variant}` arm (missing \
                 `{required}`) — it does not discriminate. Got: {calls:?}"
            );
        }
    }

    // Discrimination floor: an UNRELATED `match` in the same body is not
    // collected as a framework dispatch arm, so it can never dilute or
    // satisfy the per-arm requirement.
    let unrelated_match = r#"
        fn execute_bound_host_products(binding: B) -> u32 {
            match outcome {
                Outcome::Vue(x) => backend.admit_host_products(x),
                Outcome::Svelte(x) => x,
            }
        }
    "#;
    let unrelated_file = syn::parse_file(unrelated_match).expect("parse unrelated-match file");
    assert!(
        dispatch_arm_calls(
            &unrelated_file,
            "execute_bound_host_products",
            BOUND_DISPATCH_ENUM
        )
        .is_empty(),
        "an unrelated `match` must not be collected as a bound-request dispatch arm"
    );
}

/// NEGATIVE self-test: the guard MUST follow `self.method()` indirection one
/// level into a SAME-IMPL sibling method. A `compile_entry` that calls
/// `self.helper()` where `helper` invokes a forbidden producer would evade a
/// body-only / free-call-only scan; the method-call expansion catches it. This
/// proves the method-indirection hardening discriminates — it fails against a
/// tree that hides a producer behind a sibling method.
#[test]
fn no_hardcode_guard_follows_self_method_indirection_to_a_hidden_producer() {
    // `compile_entry` calls NO forbidden producer directly and NO free helper —
    // it only calls `self.assemble_runtime()`, a sibling method that itself
    // calls the forbidden `compile_sfc()`. A free-call-only one-level expansion
    // (a free-call-only guard) would miss this; the method-call expansion follows it.
    let hidden_via_method = r#"
        struct H;
        impl H {
            fn compile_entry(&self) -> u32 {
                self.assemble_runtime()
            }
            fn assemble_runtime(&self) -> u32 {
                let _ = compile_sfc();
                0
            }
        }
    "#;
    let file =
        syn::parse_file(hidden_via_method).expect("parse self.method()-hidden producer file");
    let bad = forbidden_calls_reachable_from(&file, "compile_entry", &FORBIDDEN_PRODUCERS);
    assert!(
        bad.contains(&"compile_sfc".to_string()),
        "the no-hardcode guard FAILED to follow `self.assemble_runtime()` indirection into the \
         sibling method that calls `compile_sfc()` — the guard does not catch a method-hidden \
         producer call. Got: {bad:?}"
    );

    // Discrimination floor: a SAME-SHAPED file whose sibling method calls a
    // NON-forbidden function must NOT be flagged (the expansion follows the
    // method but only forbidden names count).
    let clean_via_method = r#"
        struct H;
        impl H {
            fn compile_entry(&self) -> u32 {
                self.assemble_runtime()
            }
            fn assemble_runtime(&self) -> u32 {
                let _ = compile_host_products();
                0
            }
        }
    "#;
    let clean_file =
        syn::parse_file(clean_via_method).expect("parse clean method-indirection file");
    let clean = forbidden_calls_reachable_from(&clean_file, "compile_entry", &FORBIDDEN_PRODUCERS);
    assert!(
        clean.is_empty(),
        "the guard wrongly flagged a clean `self.assemble_runtime()` → `compile_host_products()` \
         indirection — it does not discriminate. Got: {clean:?}"
    );
}

/// Static guard: the host `get_ide` reader must NOT compute on read — it
/// must not call any compile / ensure / virtual-file producer. It is a pure
/// cached peek (`peek_tsx`).
#[test]
fn get_ide_is_a_pure_cached_read_no_compute() {
    let src = read_workspace_file(PIPELINE_REL);
    let file = syn::parse_file(&src).expect("parse virtual_file_pipeline.rs");
    let fns = index_free_fns(&file);
    let get_ide = fns
        .get("get_ide")
        .expect("get_ide not found — guard anchor moved");

    let mut collector = CallNameCollector {
        free_calls: Vec::new(),
        method_calls: Vec::new(),
    };
    collector.visit_block(&get_ide.block);

    // `get_ide` must never reach a compute path. These method/fn names are the
    // compile producers; their presence means `get_ide` computes on read.
    const COMPUTE_NAMES: [&str; 5] = [
        "ensure_compile_artifacts",
        "ensure_compiled",
        "ensure_ide_compiled",
        "get_virtual_file",
        "compile_entry",
    ];
    let mut all: Vec<String> = collector.free_calls;
    all.extend(collector.method_calls);
    let compute_hits: Vec<&String> = all
        .iter()
        .filter(|c| COMPUTE_NAMES.contains(&c.as_str()))
        .collect();
    assert!(
        compute_hits.is_empty(),
        "`get_ide` calls a compute path {compute_hits:?}. `get_ide` MUST stay a \
         pure cached read (`peek_tsx`); the explicit `ensure_ide_compiled` path computes."
    );
}

/// Static guard: the IDE-ensure path must NOT request
/// `VirtualNodeKind::Main`. `ensure_ide_compiled` resolves through the `Ide`
/// demand, not a Main virtual-node request.
#[test]
fn ensure_ide_compiled_never_requests_virtual_node_main() {
    let src = read_workspace_file(PIPELINE_REL);
    let file = syn::parse_file(&src).expect("parse virtual_file_pipeline.rs");
    let fns = index_free_fns(&file);
    let ensure_ide = fns
        .get("ensure_ide_compiled")
        .expect("ensure_ide_compiled not found — the explicit IDE-ensure path moved");

    // Walk the body for any path segment `Main` (the `VirtualNodeKind::Main`
    // variant). The IDE-ensure path resolves through `CompileDemand::Ide`, so
    // `Main` must never appear.
    struct MainCounter {
        main_hits: usize,
        demand_ide: bool,
    }
    impl<'ast> Visit<'ast> for MainCounter {
        fn visit_path_segment(&mut self, seg: &'ast syn::PathSegment) {
            if seg.ident == "Main" {
                self.main_hits += 1;
            }
            if seg.ident == "Ide" {
                self.demand_ide = true;
            }
            syn::visit::visit_path_segment(self, seg);
        }
    }
    let mut counter = MainCounter {
        main_hits: 0,
        demand_ide: false,
    };
    counter.visit_block(&ensure_ide.block);

    assert_eq!(
        counter.main_hits, 0,
        "`ensure_ide_compiled` references `VirtualNodeKind::Main` — it MUST NOT \
         request the runtime Main node. It resolves through `CompileDemand::Ide`, so a Main-less \
         carrier (Svelte) succeeds without a runtime Main."
    );
    assert!(
        counter.demand_ide,
        "`ensure_ide_compiled` must resolve through `CompileDemand::Ide` — the \
         `Ide` demand was not observed in its body."
    );
}
