//! Unit tests for the `$store` auto-subscription substrate: the candidate set,
//! the SCOPE-AWARE ordered subscription scan (script frames + analyzed
//! template expressions through the real `ScopeGraph`), the official
//! `store_invalid_scoped_subscription` rejects, and the demand-driven
//! admission closure. Every test asserts BOTH what IS classified and what is
//! NOT.

use oxc_allocator::Allocator;
use rustc_hash::FxHashSet;

use super::super::client_surface_imports::classify_script_imports;
use super::super::expr::{
    collect_expr_references, AnalyzedExpr, BindingInfo, BindingRuntimeKind, BindingTable,
    ScopeGraph, ScopeId,
};
use super::super::unsupported::UnsupportedSvelteRuntimeSurface;
use super::{
    collect_store_subscriptions, collect_top_level_function_names, rune_root_accessor_exemptions,
    store_base_candidates, store_dependency_closure, StoreSubscription,
};

fn candidates(instance: &str, module: Option<&str>) -> FxHashSet<String> {
    let alloc = Allocator::default();
    let imports = classify_script_imports(module, Some(instance));
    store_base_candidates(&alloc, Some(instance), &imports)
}

fn set(names: &[&str]) -> FxHashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// A minimal scope fixture: `(graph, module_scope, root_scope)` mirroring the
/// runtime lexical chain (`module → root`).
fn scope_fixture() -> (ScopeGraph, ScopeId, ScopeId) {
    let (mut graph, module) = ScopeGraph::with_root();
    let root = graph.push_scope(Some(module));
    (graph, module, root)
}

/// Analyze one template-expression text in `scope` (the same producer the
/// runtime lowering uses — `collect_expr_references` + `AnalyzedExpr::interned`).
fn analyzed(source: &str, scope: ScopeId) -> AnalyzedExpr<'_> {
    let facts = collect_expr_references(source).expect("expression parses");
    AnalyzedExpr::interned(source, scope, facts)
}

/// Run the scan with no instance script over `exprs`.
fn scan_template(
    exprs: &[AnalyzedExpr<'_>],
    graph: &ScopeGraph,
    bindings: &BindingTable,
    root: ScopeId,
    module: ScopeId,
    cands: &FxHashSet<String>,
) -> Result<Vec<StoreSubscription>, UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    collect_store_subscriptions(
        &alloc,
        None,
        exprs,
        graph,
        bindings,
        root,
        Some(module),
        cands,
    )
}

#[test]
fn candidates_cover_declarations_and_import_locals_and_rune_named_bases() {
    let instance = "import { writable } from 'svelte/store';\n\
                    const c = writable(0);\n\
                    function w(v) { return v; }\n\
                    let state = 1;\n\
                    let plain = 2;";
    let got = candidates(instance, None);
    // Declared names AND import locals are candidates.
    assert!(got.contains("c"), "top-level const is a candidate");
    assert!(got.contains("w"), "top-level function is a candidate");
    assert!(got.contains("plain"), "top-level let is a candidate");
    assert!(got.contains("writable"), "import local is a candidate");
    // A base whose accessor name is a rune-root word IS a candidate — base
    // resolution decides store-vs-rune, never the name (official emits the
    // `$state` subscription over `let state = 1` / `const state = writable(0)`).
    assert!(
        got.contains("state"),
        "a non-rune-init base whose `$name` is a rune root IS a candidate"
    );
}

#[test]
fn candidates_exclude_rune_inits_dollar_bases_and_svelte_store_derived_import() {
    // A RUNE-INITIALIZED declarator is NOT a store base (`let state = $state(0)`
    // keeps `$state` a rune — the official `get_rune(init) === null` gate).
    let got = candidates(
        "let state = $state(0);\nlet d = $derived.by(() => 1);",
        None,
    );
    assert!(
        !got.contains("state"),
        "a rune-call-initialized declarator is never a store base"
    );
    assert!(
        !got.contains("d"),
        "a rune-MEMBER-call-initialized declarator is never a store base"
    );

    // The official `$derived`-from-`'svelte/store'` special case: the import
    // LOCAL `derived` from `svelte/store` never backs a `$derived` subscription.
    let got = candidates("import { derived } from 'svelte/store';", None);
    assert!(
        !got.contains("derived"),
        "`derived` imported from 'svelte/store' is excluded"
    );
    // An ALIASED local (`derived as dd`) and the SAME local from another module
    // stay candidates.
    let got = candidates("import { derived as dd } from 'svelte/store';", None);
    assert!(got.contains("dd"), "an aliased local is a candidate");
    let got = candidates("import { derived } from './stores.js';", None);
    assert!(
        got.contains("derived"),
        "`derived` from a NON-svelte/store module is a candidate"
    );
}

#[test]
fn rune_root_exemptions_are_the_candidate_backed_accessor_names() {
    let exempt = rune_root_accessor_exemptions(&set(&["state", "derived", "plain"]));
    assert!(exempt.contains("$state"), "`state` base exempts `$state`");
    assert!(
        exempt.contains("$derived"),
        "`derived` base exempts `$derived`"
    );
    // NEGATIVE: a non-rune-root accessor never enters the exemption set, and a
    // rune root with no candidate base stays a rune.
    assert!(!exempt.contains("$plain"), "`$plain` is not a rune root");
    assert!(
        !exempt.contains("$props"),
        "no `props` base ⇒ `$props` stays a rune"
    );
}

#[test]
fn subscription_scan_is_scope_aware_and_first_seen_ordered() {
    let (graph, module, root) = scope_fixture();
    let bindings = BindingTable::new();
    let alloc = Allocator::default();
    let instance = "const a = 1;\nconst doubled = derived(a, ($a) => $a * 2);";
    let cands = set(&["a", "doubled", "derived"]);
    // The SHADOWED `$a` callback param mints NO subscription; only the template
    // `$doubled` read subscribes.
    let exprs = [analyzed("$doubled", root)];
    let subs = collect_store_subscriptions(
        &alloc,
        Some(instance),
        &exprs,
        &graph,
        &bindings,
        root,
        Some(module),
        &cands,
    )
    .expect("no scoped violation");
    let names: Vec<&str> = subs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["$doubled"],
        "the shadowed `$a` callback param must NOT subscribe"
    );
    assert_eq!(subs[0].base(), "doubled");

    // FIRST-SEEN order across template expressions: `{$b}` before `{$a}` mints
    // `$b` then `$a` (reference order, NOT declaration order).
    let exprs = [analyzed("$b", root), analyzed("$a", root)];
    let subs = scan_template(&exprs, &graph, &bindings, root, module, &set(&["a", "b"])).unwrap();
    let names: Vec<&str> = subs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["$b", "$a"], "accessor order is first-seen");
}

#[test]
fn subscription_scan_admits_rune_named_bases_and_excludes_magic_and_undeclared() {
    let (graph, module, root) = scope_fixture();
    let bindings = BindingTable::new();
    let cands = set(&["c", "state"]);
    let exprs = [
        analyzed("$$props", root),
        analyzed("$state", root),
        analyzed("$missing", root),
        analyzed("$c", root),
    ];
    let subs = scan_template(&exprs, &graph, &bindings, root, module, &cands).unwrap();
    let names: Vec<&str> = subs.iter().map(|s| s.name.as_str()).collect();
    // `$state` over a DECLARED base subscribes (base resolution, not the name);
    // `$$props` (magic) and `$missing` (no declared base) never subscribe.
    assert_eq!(
        names,
        vec!["$state", "$c"],
        "`$$`-prefixed and undeclared-base names never subscribe; a declared \
         rune-root-named base DOES"
    );
}

#[test]
fn subscription_scan_counts_writes_and_instance_script_references() {
    let (graph, module, root) = scope_fixture();
    let bindings = BindingTable::new();
    let alloc = Allocator::default();
    // A WRITE target (`$c = 5`) inside an instance-script function body is a
    // subscription (the instance program is scanned before template exprs).
    let instance = "const c = make();\nfunction set5() { $c = 5; }";
    let subs = collect_store_subscriptions(
        &alloc,
        Some(instance),
        &[],
        &graph,
        &bindings,
        root,
        Some(module),
        &set(&["c", "make"]),
    )
    .unwrap();
    let names: Vec<&str> = subs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["$c"], "a store WRITE is a subscription");
}

#[test]
fn script_base_shadow_rejects_scoped_subscription() {
    let (graph, module, root) = scope_fixture();
    let bindings = BindingTable::new();
    let alloc = Allocator::default();
    // `function f(x) { return $x; }` — the function PARAM owns the base `x` at
    // the `$x` reference: the official `store_invalid_scoped_subscription`.
    let instance = "const x = writable(1);\nfunction f(x) { return $x; }";
    let err = collect_store_subscriptions(
        &alloc,
        Some(instance),
        &[],
        &graph,
        &bindings,
        root,
        Some(module),
        &set(&["x", "writable"]),
    )
    .expect_err("the base-shadowed `$x` must reject");
    assert!(
        matches!(
            &err,
            UnsupportedSvelteRuntimeSurface::StoreScopedSubscription { name, .. } if name == "$x"
        ),
        "expected the scoped-subscription reject, got {err:?}"
    );

    // CONTROL: the same shape WITHOUT the param shadow subscribes.
    let instance = "const x = writable(1);\nfunction f() { return $x; }";
    let subs = collect_store_subscriptions(
        &alloc,
        Some(instance),
        &[],
        &graph,
        &bindings,
        root,
        Some(module),
        &set(&["x", "writable"]),
    )
    .unwrap();
    assert_eq!(subs.len(), 1, "the unshadowed `$x` subscribes");
    assert_eq!(subs[0].name, "$x");
}

#[test]
fn template_block_base_shadow_rejects_scoped_subscription() {
    // Model `{#each items as x}{$x}{/each}` over a top-level store `x`: the
    // each BODY scope declares `x` as an `EachSignal`, and the analyzed
    // expression `$x` is evaluated IN that body scope.
    let (mut graph, module, root) = scope_fixture();
    let mut bindings = BindingTable::new();
    let body = graph.push_scope(Some(root));
    let alias = bindings.push(BindingInfo {
        name: "x".to_string(),
        scope: body,
        kind: BindingRuntimeKind::EachSignal,
        state: None,
    });
    graph.declare(body, "x", alias);
    let cands = set(&["x"]);

    let exprs = [analyzed("$x", body)];
    let err = scan_template(&exprs, &graph, &bindings, root, module, &cands)
        .expect_err("the each-alias base shadow must reject");
    assert!(
        matches!(
            &err,
            UnsupportedSvelteRuntimeSurface::StoreScopedSubscription { name, .. } if name == "$x"
        ),
        "expected the scoped-subscription reject, got {err:?}"
    );

    // CONTROL: the SAME body scope subscribing a NON-shadowed store `$y` is
    // fine (only a base-name collision rejects).
    let exprs = [analyzed("$y", body)];
    let subs = scan_template(&exprs, &graph, &bindings, root, module, &set(&["x", "y"])).unwrap();
    let names: Vec<&str> = subs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["$y"],
        "a non-shadowed base subscribes from a block body"
    );
}

#[test]
fn expression_internal_base_shadow_rejects_scoped_subscription() {
    // `onclick={(x) => $x}` — the arrow param INSIDE the analyzed expression
    // owns the base at the `$x` read (the per-reference
    // `store_base_locally_bound` fact).
    let (graph, module, root) = scope_fixture();
    let bindings = BindingTable::new();
    let exprs = [analyzed("(x) => $x", root)];
    let err = scan_template(&exprs, &graph, &bindings, root, module, &set(&["x"]))
        .expect_err("the expression-internal base shadow must reject");
    assert!(
        matches!(
            &err,
            UnsupportedSvelteRuntimeSurface::StoreScopedSubscription { name, .. } if name == "$x"
        ),
        "expected the scoped-subscription reject, got {err:?}"
    );

    // CONTROL: a LITERAL `$x` param (`($x) => $x`) is a plain local — the inner
    // read is binder-pruned from the reference facts, so nothing subscribes and
    // nothing rejects.
    let exprs = [analyzed("($x) => $x", root)];
    let subs = scan_template(&exprs, &graph, &bindings, root, module, &set(&["x"])).unwrap();
    assert!(
        subs.is_empty(),
        "a literal `$x` local read neither subscribes nor rejects"
    );
}

#[test]
fn dependency_closure_is_seeded_by_subscriptions_only() {
    let alloc = Allocator::default();
    let instance = "function w(v) { return v; }\n\
                    const c = w(0);\n\
                    const unrelated = make();";
    // Seeded by the subscribed base `c`: the const `c` is admitted AND the
    // factory `w` its init references; the UNRELATED const is NOT admitted.
    let (consts, fns, classes) = store_dependency_closure(&alloc, Some(instance), &set(&["c"]));
    assert!(consts.contains("c"), "the subscribed const is admitted");
    assert!(
        fns.contains("w"),
        "the factory the init references is admitted"
    );
    assert!(
        !consts.contains("unrelated"),
        "an arbitrary call-initialized const with no subscription stays refused"
    );
    assert!(
        classes.is_empty(),
        "no class dependency ⇒ no class admission"
    );

    // NEGATIVE: with NO subscription the closure admits NOTHING (the demand
    // boundary — `const x = make()` stays refused).
    let (consts, fns, classes) =
        store_dependency_closure(&alloc, Some(instance), &FxHashSet::default());
    assert!(
        consts.is_empty() && fns.is_empty() && classes.is_empty(),
        "no seed ⇒ no admission"
    );
}

#[test]
fn dependency_closure_admits_a_transitive_store_class() {
    let alloc = Allocator::default();
    // `$c` subscribes over `c`; `c`'s init `new S()` names the local store
    // CLASS `S` — admitted transitively, the SAME demand-driven way const/fn
    // dependencies are. An UNRELATED top-level class stays refused.
    let instance = "class S { subscribe(fn) { fn(1); return () => {}; } }\n\
                    class Unrelated {}\n\
                    const c = new S();";
    let (consts, fns, classes) = store_dependency_closure(&alloc, Some(instance), &set(&["c"]));
    assert!(
        consts.contains("c"),
        "the subscribed const source is admitted"
    );
    assert!(
        classes.contains("S"),
        "the store class the init `new S()` references is admitted"
    );
    assert!(
        !classes.contains("Unrelated"),
        "a class unreachable from any subscription stays refused (out of the store-subscription scope)"
    );
    assert!(fns.is_empty(), "no function dependency here");

    // NEGATIVE: with NO subscription the class admits NOTHING.
    let (_c, _f, classes) = store_dependency_closure(&alloc, Some(instance), &FxHashSet::default());
    assert!(
        classes.is_empty(),
        "no seed ⇒ the store class is not admitted"
    );
}

#[test]
fn dependency_closure_admits_transitive_store_dependencies() {
    let alloc = Allocator::default();
    // `$doubled` subscribes; `doubled`'s init references `a` (a store DEP) and
    // the import `derived` (not a top-level const/function — contributes
    // nothing); `a`'s init references `writable` (an import — nothing).
    let instance = "import { writable, derived } from 'svelte/store';\n\
                    const a = writable(1);\n\
                    const doubled = derived(a, ($a) => $a * 2);";
    let (consts, fns, classes) =
        store_dependency_closure(&alloc, Some(instance), &set(&["doubled"]));
    assert!(consts.contains("doubled"));
    assert!(
        consts.contains("a"),
        "the un-subscribed store DEPENDENCY `a` is admitted transitively"
    );
    assert!(fns.is_empty(), "imports are not function admissions");
    assert!(classes.is_empty(), "no class dependency here");
}

#[test]
fn top_level_function_names_are_collected() {
    let alloc = Allocator::default();
    let instance = "function inc() {}\nconst arrow = () => {};";
    let names = collect_top_level_function_names(&alloc, Some(instance));
    assert!(names.contains("inc"));
    // NEGATIVE: a const-arrow is NOT a function declaration referent.
    assert!(!names.contains("arrow"));
}
