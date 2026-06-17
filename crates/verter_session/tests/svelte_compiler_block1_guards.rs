//! Carrier-runtime-routing architecture guards (native-Svelte foundation).
//!
//! These pin the foundation of the native-Svelte compiler program: the
//! runtime compile in `compile_entry` is routed through the
//! `CarrierCompilerRegistry`, never the hardcoded Vue producer, and the
//! §APIDECISION (ruling B) IDE-ensure path is implemented as an explicit
//! demand enum — `ensure_ide_compiled` never requests `VirtualNodeKind::Main`
//! and `get_ide` never computes on read.
//!
//! The no-hardcode guard is an AST/`syn` scan (NOT a substring scan): it
//! parses `virtual_file_pipeline.rs`, walks `compile_entry`'s body plus the
//! one-level local helpers it reaches, and asserts none calls the hardcoded
//! Vue free functions (`compile` / `compile_from_parsed` / `compile_sfc` /
//! `vue_parse`); it also inspects the file's `use` declarations for any alias,
//! rename, or glob that would re-bind one of those symbols under a new name.
//! A negative self-test proves the guard catches a renamed import.

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

/// The hardcoded Vue runtime producers `compile_entry` must NOT call.
const FORBIDDEN_PRODUCERS: [&str; 4] =
    ["compile", "compile_from_parsed", "compile_sfc", "vue_parse"];

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

/// Index every free `fn` in a parsed file by name (so a `compile_entry` body's
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

/// Walk `compile_entry`'s body + its one-level local callees and return the set
/// of forbidden producers invoked. Shared by the live guard and the negative
/// self-test (which runs it over a synthetic file).
fn forbidden_producer_calls_in_compile_entry(file: &File) -> Vec<String> {
    let fns = index_free_fns(file);
    let entry = fns
        .get("compile_entry")
        .expect("compile_entry not found in virtual_file_pipeline.rs — guard anchor moved");

    let mut collector = CallNameCollector {
        free_calls: Vec::new(),
        method_calls: Vec::new(),
    };
    collector.visit_block(&entry.block);

    // One-level local helper expansion: any free fn `compile_entry` calls is
    // also scanned (an indirection like `fn h() { compile_sfc(...) }` reached
    // from the entry body would otherwise evade a body-only scan).
    let mut reached: Vec<String> = collector.free_calls.clone();
    let mut helper_calls: Vec<String> = Vec::new();
    for callee in &reached.clone() {
        if let Some(f) = fns.get(callee) {
            let mut hc = CallNameCollector {
                free_calls: Vec::new(),
                method_calls: Vec::new(),
            };
            hc.visit_block(&f.block);
            helper_calls.extend(hc.free_calls.clone());
            reached.extend(hc.free_calls);
        }
    }

    let mut all_calls: Vec<String> = collector.free_calls;
    all_calls.extend(collector.method_calls);
    all_calls.extend(helper_calls);

    all_calls
        .into_iter()
        .filter(|c| FORBIDDEN_PRODUCERS.contains(&c.as_str()))
        .collect()
}

/// THE no-hardcode guard. `compile_entry` routes the runtime compile through
/// the carrier registry — it must NOT call the hardcoded Vue producers, and
/// `virtual_file_pipeline.rs` must NOT import them under any alias / rename /
/// glob.
#[test]
fn compile_entry_routes_through_carrier_registry_not_hardcoded_vue() {
    let src = read_workspace_file(PIPELINE_REL);
    let file = syn::parse_file(&src).expect("parse virtual_file_pipeline.rs");

    // (1) No forbidden producer is CALLED in compile_entry (or its local helpers).
    let bad_calls = forbidden_producer_calls_in_compile_entry(&file);
    assert!(
        bad_calls.is_empty(),
        "carrier routing: `compile_entry` calls the hardcoded Vue producer(s) {bad_calls:?}. \
         The runtime compile MUST route through `CarrierCompilerRegistry::compile_bundle` — \
         delete the direct `compile` / `compile_from_parsed` / `compile_sfc` / `vue_parse` use."
    );

    // (2) The file must not IMPORT a forbidden producer under any name. A glob
    // import crossing the compiler `compile` module (`use
    // verter_compiler::compile::*`) could re-bind `compile_sfc` /
    // `compile_from_parsed` / `compile` into scope without a named `use`, so
    // such a glob is itself a violation here (the file imports its compiler
    // symbols explicitly). An unrelated glob (`crate::types::*`) is fine.
    let bindings = all_use_bindings(&file);
    let imported_forbidden: Vec<&String> = bindings
        .idents
        .iter()
        .filter(|b| FORBIDDEN_PRODUCERS.contains(&b.as_str()))
        .collect();
    assert!(
        imported_forbidden.is_empty(),
        "carrier routing: `virtual_file_pipeline.rs` imports the hardcoded Vue producer(s) \
         {imported_forbidden:?} — even an alias / rename re-binds them. The runtime \
         producer is reached through the carrier registry, not a direct import."
    );
    assert!(
        !bindings.compiler_compile_glob,
        "carrier routing: `virtual_file_pipeline.rs` has a glob `use` crossing `verter_compiler::compile` \
         — a glob there could re-bind a hardcoded Vue producer (`compile_sfc`, …) into scope \
         invisibly. Import compiler symbols explicitly so this guard can verify the forbidden \
         producers are absent."
    );
}

/// NEGATIVE self-test: the guard MUST catch a renamed import + aliased call of
/// a forbidden producer. This proves the guard discriminates (it fails against
/// a violating tree) — without it the positive test could pass vacuously.
#[test]
fn no_hardcode_guard_catches_a_renamed_producer_import_and_call() {
    // A synthetic `virtual_file_pipeline.rs` that RENAMES the forbidden
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
    let bad = forbidden_producer_calls_in_compile_entry(&direct_file);
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

/// §APIDECISION static guard: the host `get_ide` reader must NOT compute on
/// read — it must not call any compile / ensure / virtual-file producer. It is
/// a pure cached peek (`peek_tsx`).
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
        "§APIDECISION: `get_ide` calls a compute path {compute_hits:?}. `get_ide` MUST stay a \
         pure cached read (`peek_tsx`); the explicit `ensure_ide_compiled` path computes."
    );
}

/// §APIDECISION static guard: the IDE-ensure path must NOT request
/// `VirtualNodeKind::Main`. `ensure_ide_compiled` resolves through the `Ide`
/// demand, not a Main virtual-node request.
#[test]
fn ensure_ide_compiled_never_requests_virtual_node_main() {
    let src = read_workspace_file(PIPELINE_REL);
    let file = syn::parse_file(&src).expect("parse virtual_file_pipeline.rs");
    let fns = index_free_fns(&file);
    let ensure_ide = fns
        .get("ensure_ide_compiled")
        .expect("ensure_ide_compiled not found — APIDECISION ruling B not implemented");

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
        "§APIDECISION: `ensure_ide_compiled` references `VirtualNodeKind::Main` — it MUST NOT \
         request the runtime Main node. It resolves through `CompileDemand::Ide`, so a Main-less \
         carrier (Svelte) succeeds without a runtime Main."
    );
    assert!(
        counter.demand_ide,
        "§APIDECISION: `ensure_ide_compiled` must resolve through `CompileDemand::Ide` — the \
         `Ide` demand was not observed in its body."
    );
}
