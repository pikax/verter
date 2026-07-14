//! Tests for the Svelte runtime IR substrate: the `$state` four/five-way
//! classification, the scope-aware shadowing invariant, the each/await signal
//! bindings, the declaration-tag-vs-`{@const}` distinction, the fragment-flag +
//! comment-anchor static-template rules, the delegated-event policy, the
//! per-block IR-shape, and the topology summary diffed against the conformance
//! oracle goldens on the IR-determinable axes.

use oxc_allocator::Allocator;

use super::expr::{BindingRuntimeKind, StateLowering};
use super::helpers::SvelteHelper;
use super::html::{AnchorReason, TemplateFactory, TemplateFlag};
use super::ir::{BlockIr, IrNode, SpecialKind, TagIr};
use super::{
    lower_parsed_svelte_to_ir, plan_client_topology, plan_static_templates, SvelteRuntimeOptions,
};
use crate::svelte::parser::parse_svelte;

/// Lower a Svelte source into the runtime IR with default options, panicking on a
/// lowering error (the fixtures are well-formed).
fn lower<'a>(source: &'a str, alloc: &'a Allocator) -> super::ir::SvelteRuntimeIr<'a> {
    let parsed = parse_svelte(source);
    lower_parsed_svelte_to_ir(source, &parsed, &SvelteRuntimeOptions::default(), alloc)
        .expect("lowering succeeds for a well-formed fixture")
}

/// Lower a Svelte source returning the `Result`, so a negative test can assert the
/// collected lowering DIAGNOSTICS (e.g. the `attribute_invalid_event_handler`
/// equivalent) rather than panicking.
fn lower_result<'a>(
    source: &'a str,
    alloc: &'a Allocator,
) -> Result<super::ir::SvelteRuntimeIr<'a>, super::RuntimeLoweringErrors> {
    let parsed = parse_svelte(source);
    lower_parsed_svelte_to_ir(source, &parsed, &SvelteRuntimeOptions::default(), alloc)
}

/// Lower `source` under an explicit component `name` option and return the derived
/// component-function name (the svelte `Scope.generate` deconfliction result).
fn component_name_with_option(source: &str, name: &str, alloc: &Allocator) -> String {
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        name: Some(name.to_string()),
        ..SvelteRuntimeOptions::default()
    };
    lower_parsed_svelte_to_ir(source, &parsed, &opts, alloc)
        .expect("lowering succeeds for a well-formed fixture")
        .component
        .name
}

#[test]
fn authored_declarations_reserve_the_component_name() {
    // Every script + template declaration form reserves the component-function name:
    // svelte@5.56.3 renders a declared `Foo` under component name `Foo` as `Foo_1`.
    // Each row's `Foo` appears ONLY as the declaration (never template-referenced),
    // so it isolates the declaration path — not the free-reference fold.
    let alloc = Allocator::default();
    let cases: &[(&str, &str)] = &[
        (
            "export let prop",
            "<script>export let Foo;</script>\n<div>hi</div>",
        ),
        (
            "export function",
            "<script>export function Foo() {}</script>\n<div>hi</div>",
        ),
        (
            "export class",
            "<script>export class Foo {}</script>\n<div>hi</div>",
        ),
        (
            "export const",
            "<script>export const Foo = 1;</script>\n<div>hi</div>",
        ),
        (
            "instance import",
            "<script>import Foo from './x.js';</script>\n<div>hi</div>",
        ),
        (
            "module import",
            "<script module>import Foo from './x.js';</script>\n<div>hi</div>",
        ),
        (
            "props destructure",
            "<script>let { Foo } = $props();</script>\n<div>hi</div>",
        ),
        (
            "each item",
            "<script>let items = [];</script>\n{#each items as Foo}<span></span>{/each}",
        ),
        (
            "each index",
            "<script>let items = [];</script>\n{#each items as it, Foo}<span></span>{/each}",
        ),
        (
            "await then",
            "<script>let p = Promise.resolve(1);</script>\n{#await p then Foo}<span></span>{/await}",
        ),
        (
            "await catch",
            "<script>let p = Promise.resolve(1);</script>\n{#await p}<span></span>{:catch Foo}<span></span>{/await}",
        ),
        (
            "snippet name",
            "<script>let x = 1;</script>\n{#snippet Foo()}<span></span>{/snippet}\n<span>{x}</span>",
        ),
        (
            "slot let: binding",
            "<script>import C from './C.svelte';</script>\n<C let:Foo><span></span></C>",
        ),
        (
            "@const tag",
            "<script>let x = 1;</script>\n{#if x}{@const Foo = x + 1}<span>{Foo}</span>{/if}",
        ),
    ];
    for (label, src) in cases {
        assert_eq!(
            component_name_with_option(src, "Foo", &alloc),
            "Foo_1",
            "the {label} declaration `Foo` must reserve the component name (expected `Foo_1`)"
        );
    }
}

/// Resolve the runtime kind of a top-level (root-scope) binding by name.
fn root_binding_kind(ir: &super::ir::SvelteRuntimeIr, name: &str) -> Option<BindingRuntimeKind> {
    let root_scope = ir.root_scope().scope;
    ir.analysis
        .bindings
        .resolve_kind(&ir.analysis.scopes, root_scope, name)
}

/// The `StateLowering` recorded for a named `$state` binding.
fn state_lowering(ir: &super::ir::SvelteRuntimeIr, name: &str) -> Option<StateLowering> {
    ir.analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == name)
        .and_then(|b| b.state.map(|s| s.lowering))
}

// ---------------------------------------------------------------------------
// Test 1 — the $state four/five-way classification.
// Discriminates against: a one-decision "every $state is a signal" classifier.
//
// The lowering is WRITE-gated (verified against svelte@5.56.3): a $state that is
// never WRITTEN (neither reassigned nor deep-mutated) collapses to a plain `let`,
// regardless of where/whether it is read (template, $derived, $effect, a function
// body). Only a WRITE makes a $state reactive.
// ---------------------------------------------------------------------------

#[test]
fn effect_family_helpers_are_registered() {
    // The four effect-family runtime helpers each map to the official
    // `svelte/internal/client` export name. The three NEW family members
    // (`user_pre_effect` / `effect_root` / `effect_tracking`) must be DISTINCT
    // helper families (not re-labels of `user_effect`).
    assert_eq!(SvelteHelper::UserEffect.ident(), "user_effect");
    assert_eq!(SvelteHelper::UserPreEffect.ident(), "user_pre_effect");
    assert_eq!(SvelteHelper::EffectRoot.ident(), "effect_root");
    assert_eq!(SvelteHelper::EffectTracking.ident(), "effect_tracking");
    // NEGATIVE: the four families are pairwise distinct mask bits (a shared bit
    // would alias membership queries).
    let helpers = [
        SvelteHelper::UserEffect,
        SvelteHelper::UserPreEffect,
        SvelteHelper::EffectRoot,
        SvelteHelper::EffectTracking,
    ];
    for (i, a) in helpers.iter().enumerate() {
        for b in helpers.iter().skip(i + 1) {
            assert_ne!(a.bit(), b.bit(), "{a:?} and {b:?} share a mask bit");
        }
    }
}

#[test]
fn state_classification_is_four_way_plus_raw() {
    let alloc = Allocator::default();

    // (a) never written, read once statically in the script → PlainLet.
    let src_plain = "<script>\n\tlet n = $state(0);\n\tconsole.log(n);\n</script>\n<p>static</p>";
    let ir = lower(src_plain, &alloc);
    assert_eq!(
        state_lowering(&ir, "n"),
        Some(StateLowering::PlainLet),
        "a never-written $state must lower to a plain let"
    );

    // (b) written primitive (reassigned via a template handler) → StateSignal.
    // (`let count=$state(0)` read-only via `{count}` with NO write compiles to a
    // plain `let count=0` in svelte@5.56.3 — a write is required for the signal.)
    let src_signal = "<script>\n\tlet count = $state(0);\n</script>\n<button onclick={() => count++}>{count}</button>";
    let ir = lower(src_signal, &alloc);
    assert_eq!(
        state_lowering(&ir, "count"),
        Some(StateLowering::StateSignal),
        "a written primitive $state must be a signal"
    );

    // (c) object/array, deep-mutated but never reassigned → BareProxy.
    let src_proxy = "<script>\n\tlet box = $state({ a: 1, b: 2 });\n\tfunction bump(){ box.a += 1; }\n</script>\n<button>{box.a}</button>";
    let ir = lower(src_proxy, &alloc);
    assert_eq!(
        state_lowering(&ir, "box"),
        Some(StateLowering::BareProxy),
        "an object $state deep-mutated but never reassigned must be a bare proxy"
    );

    // (d) object/array, reassigned → StateProxy.
    let src_state_proxy = "<script>\n\tlet box = $state({ a: 1 });\n\tfunction reset(){ box = { a: 0 }; }\n</script>\n<button>{box.a}</button>";
    let ir = lower(src_state_proxy, &alloc);
    assert_eq!(
        state_lowering(&ir, "box"),
        Some(StateLowering::StateProxy),
        "a reassigned object $state must be $.state($.proxy(...))"
    );

    // (e) $state.raw reassigned → RawStateSignal.
    let src_raw = "<script>\n\tlet box = $state.raw({ a: 1 });\n\tfunction reset(){ box = { a: 0 }; }\n</script>\n<button>{box.a}</button>";
    let ir = lower(src_raw, &alloc);
    assert_eq!(
        state_lowering(&ir, "box"),
        Some(StateLowering::RawStateSignal),
        "a reassigned $state.raw must be a bare signal (no proxy)"
    );
}

// ---------------------------------------------------------------------------
// Test 1b — write-gated classification (svelte@5.56.3 empirical law).
//
// F5: deep-mutated-only object (never reassigned, never read) → BareProxy.
//     (official: `let o = $.proxy({a:1})`.) Discriminates against the predicate
//     that ignores the computed `deep_mutated` flag and returns PlainLet.
// F6: reassigned-never-read primitive → StateSignal. (official: `let n=$.state(0)`.)
// fn-body-read-only: read only inside a non-reactive function body, no write →
//     PlainLet (official: `let n = 0`). Discriminates against the parenthetical
//     "a read in a function body the signal escapes into counts" — empirically
//     FALSE for the never-written case.
// ---------------------------------------------------------------------------

#[test]
fn deep_mutated_only_object_is_bare_proxy() {
    let alloc = Allocator::default();
    // `o` is deep-mutated in a handler, never reassigned, never read in the
    // template. svelte@5.56.3 emits `let o = $.proxy({a:1})`. The BareProxy comes
    // from the PROXIABLE object-literal init (proxy is unconditional for a
    // proxiable init), NOT from the deep mutation.
    let src =
        "<script>\n\tlet o = $state({ a: 1 });\n</script>\n<button onclick={() => o.a++}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "o"),
        Some(StateLowering::BareProxy),
        "a proxiable-init object $state (deep-mutated, never reassigned) is a bare proxy (NOT PlainLet)"
    );
    assert_ne!(
        state_lowering(&ir, "o"),
        Some(StateLowering::PlainLet),
        "a proxiable object init must be $.proxy(...), never a plain let"
    );
    assert_ne!(
        state_lowering(&ir, "o"),
        Some(StateLowering::StateProxy),
        "a never-reassigned object $state must NOT gain the $.state(...) wrapper"
    );
}

// ---------------------------------------------------------------------------
// G2 — the proxy decision is `should_proxy(init)` (init SHAPE), INDEPENDENT of
// reads/writes/mutations. These cases were derived empirically against
// svelte@5.56.3 (generate:'client', runes, non-dev) and discriminate against the
// pre-fix model (proxy gated on a `deep_mutated` flag; never-written object →
// PlainLet; `.push()` / destructuring-reassign not detected).
// ---------------------------------------------------------------------------

#[test]
fn never_written_array_state_is_still_bare_proxy() {
    let alloc = Allocator::default();
    // `let arr = $state([1, 2])` never written anywhere. svelte@5.56.3 emits
    // `let arr = $.proxy([1, 2])` — a proxiable init is ALWAYS proxied, even with
    // zero writes. FAILS against the pre-fix "never written object/array →
    // PlainLet" rule.
    let src = "<script>\n\tlet arr = $state([1, 2]);\n</script>\n<p>{arr.length}</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "arr"),
        Some(StateLowering::BareProxy),
        "a never-written array $state is still a bare proxy"
    );
    assert_ne!(
        state_lowering(&ir, "arr"),
        Some(StateLowering::PlainLet),
        "a proxiable (array) init must NOT collapse to a plain let"
    );
}

#[test]
fn push_only_array_state_is_bare_proxy() {
    let alloc = Allocator::default();
    // `let items = $state([]); items.push(1)` — mutated ONLY via a method call,
    // never reassigned, never template-read. svelte@5.56.3 emits
    // `let items = $.proxy([])`. The pre-fix code mis-lowered this to PlainLet
    // (`.push()` was not detected; the array init was not unconditionally proxied).
    let src =
        "<script>\n\tlet items = $state([]);\n\tfunction add(){ items.push(1); }\n</script>\n<button onclick={add}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "items"),
        Some(StateLowering::BareProxy),
        "a push-only array $state is a bare proxy"
    );
    assert_ne!(
        state_lowering(&ir, "items"),
        Some(StateLowering::PlainLet),
        "a push-only array $state must NOT lower to a plain let"
    );
}

#[test]
fn method_mutated_object_state_is_bare_proxy() {
    let alloc = Allocator::default();
    // `let map = $state(new Map()); map.set('a', 1)` — `new Map()` is proxiable,
    // mutated only via a method call. svelte@5.56.3 emits `$.proxy(new Map())`.
    let src =
        "<script>\n\tlet map = $state(new Map());\n\tfunction add(){ map.set('a', 1); }\n</script>\n<button onclick={add}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "map"),
        Some(StateLowering::BareProxy),
        "a method-mutated object $state is a bare proxy"
    );
}

#[test]
fn element_write_only_array_state_is_bare_proxy() {
    let alloc = Allocator::default();
    // `arr[0] = 9` — an element (computed-member) write, never reassigned.
    // svelte@5.56.3 emits `$.proxy([1])`.
    let src =
        "<script>\n\tlet arr = $state([1]);\n\tfunction f(){ arr[0] = 9; }\n</script>\n<button onclick={f}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "arr"),
        Some(StateLowering::BareProxy),
        "an element-write-only array $state is a bare proxy (never a signal)"
    );
    assert_ne!(
        state_lowering(&ir, "arr"),
        Some(StateLowering::StateProxy),
        "an element write is NOT a reassignment — no $.state(...) wrapper"
    );
}

#[test]
fn object_destructuring_reassign_makes_state_signal() {
    let alloc = Allocator::default();
    // `({ count } = obj)` is a REASSIGNMENT of `count`. With a PRIMITIVE init,
    // svelte@5.56.3 emits `let count = $.state(0)` (StateSignal). FAILS against the
    // pre-fix code, which did not detect destructuring-assignment targets as
    // reassignments (so `count` stayed PlainLet).
    let src =
        "<script>\n\tlet count = $state(0);\n\tfunction f(){ ({ count } = { count: 5 }); }\n</script>\n<button onclick={f}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "count"),
        Some(StateLowering::StateSignal),
        "an object-destructuring reassignment of a primitive $state is a signal"
    );
    assert_ne!(
        state_lowering(&ir, "count"),
        Some(StateLowering::PlainLet),
        "a destructuring-assignment target is a write — it must NOT stay a plain let"
    );
}

#[test]
fn array_destructuring_reassign_of_array_state_is_state_proxy() {
    let alloc = Allocator::default();
    // `[arr] = [5]` is a REASSIGNMENT of `arr`. With a proxiable (array) init,
    // svelte@5.56.3 emits `let arr = $.state($.proxy([1]))` (StateProxy).
    let src =
        "<script>\n\tlet arr = $state([1]);\n\tfunction f(){ [arr] = [5]; }\n</script>\n<button onclick={f}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "arr"),
        Some(StateLowering::StateProxy),
        "an array-destructuring reassignment of an array $state is $.state($.proxy(...))"
    );
}

#[test]
fn template_destructuring_reassign_attributed_to_state() {
    let alloc = Allocator::default();
    // The destructuring reassignment lives in a TEMPLATE handler expression. The
    // template-side reference collector must attribute it as a reassignment of the
    // outer primitive `count` → StateSignal.
    let src = "<script>\n\tlet count = $state(0);\n</script>\n<button onclick={() => ({ count } = { count: 1 })}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "count"),
        Some(StateLowering::StateSignal),
        "a template destructuring-assignment reassignment makes a primitive $state a signal"
    );
}

#[test]
fn call_init_state_is_proxiable_even_when_unwritten() {
    let alloc = Allocator::default();
    // `$state(makeThing())` — a CALL init is proxiable (negative-list default-true).
    // Never written → BareProxy (`$.proxy(makeThing())`), NOT PlainLet, NOT
    // StateSignal. FAILS against the pre-fix "Unknown shape → StateSignal" rule.
    let src = "<script>\n\tfunction makeThing(){ return {}; }\n\tlet x = $state(makeThing());\n</script>\n<p>hi</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "x"),
        Some(StateLowering::BareProxy),
        "a call-init $state is proxiable even when never written"
    );
    assert_ne!(
        state_lowering(&ir, "x"),
        Some(StateLowering::StateSignal),
        "a never-written call-init $state must NOT be a $.state(...) signal"
    );
}

#[test]
fn binary_init_state_is_not_proxiable() {
    let alloc = Allocator::default();
    // `$state(1 + 2)` — a BINARY expression is on the non-proxiable list. Never
    // written → PlainLet (`let x = 1 + 2`). Reassigned → StateSignal (no proxy).
    let src_plain = "<script>\n\tlet x = $state(1 + 2);\n</script>\n<p>hi</p>";
    let ir = lower(src_plain, &alloc);
    assert_eq!(
        state_lowering(&ir, "x"),
        Some(StateLowering::PlainLet),
        "a never-written binary-init $state is a plain let"
    );
    assert_ne!(
        state_lowering(&ir, "x"),
        Some(StateLowering::BareProxy),
        "a binary-expression init is NOT proxiable"
    );

    let src_signal =
        "<script>\n\tlet x = $state(1 + 2);\n\tfunction f(){ x = 9; }\n</script>\n<button onclick={f}>x</button>";
    let ir2 = lower(src_signal, &alloc);
    assert_eq!(
        state_lowering(&ir2, "x"),
        Some(StateLowering::StateSignal),
        "a reassigned binary-init $state is a bare signal (no proxy)"
    );
}

#[test]
fn raw_state_never_proxies_and_only_signals_when_reassigned() {
    let alloc = Allocator::default();
    // `$state.raw([])` never reassigned, only method-mutated → PlainLet (`let o =
    // []`). svelte@5.56.3 never proxies a raw state and only wraps it in
    // `$.state(...)` when reassigned. FAILS against the pre-fix code, which lowered
    // ANY written (incl. deep-mutated) raw state to RawStateSignal.
    let src_plain =
        "<script>\n\tlet o = $state.raw([]);\n\tfunction f(){ o.push(1); }\n</script>\n<button onclick={f}>x</button>";
    let ir = lower(src_plain, &alloc);
    assert_eq!(
        state_lowering(&ir, "o"),
        Some(StateLowering::PlainLet),
        "a method-mutated-only $state.raw is a plain let (raw never proxies, not reassigned)"
    );
    assert_ne!(
        state_lowering(&ir, "o"),
        Some(StateLowering::RawStateSignal),
        "a non-reassigned $state.raw must NOT be a signal"
    );

    let src_signal =
        "<script>\n\tlet o = $state.raw([]);\n\tfunction f(){ o = [1]; }\n</script>\n<button onclick={f}>x</button>";
    let ir2 = lower(src_signal, &alloc);
    assert_eq!(
        state_lowering(&ir2, "o"),
        Some(StateLowering::RawStateSignal),
        "a reassigned $state.raw is a bare signal (no proxy)"
    );
}

#[test]
fn identifier_init_one_hop_follow_to_literal_is_not_proxiable() {
    let alloc = Allocator::default();
    // `let base = 5; let x = $state(base)` — the identifier follows ONE hop to a
    // non-reassigned literal binding → not proxiable. Never written → PlainLet
    // (`let x = base`). svelte@5.56.3 verified.
    let src = "<script>\n\tlet base = 5;\n\tlet x = $state(base);\n</script>\n<p>hi</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "x"),
        Some(StateLowering::PlainLet),
        "an identifier init resolving (one hop) to a literal is not proxiable"
    );

    // Control: `let base = {}` (proxiable target) → the follow yields proxiable →
    // BareProxy.
    let src2 = "<script>\n\tlet base = { a: 1 };\n\tlet x = $state(base);\n</script>\n<p>hi</p>";
    let ir2 = lower(src2, &alloc);
    assert_eq!(
        state_lowering(&ir2, "x"),
        Some(StateLowering::BareProxy),
        "an identifier init resolving (one hop) to an object literal is proxiable"
    );
}

#[test]
fn reassigned_identifier_intermediate_blocks_proxy_follow() {
    let alloc = Allocator::default();
    // `let base = 5; base = 6; let x = $state(base)` — `base` is reassigned, so the
    // proxy follow is BLOCKED and `x`'s init stays proxiable → BareProxy
    // (`$.proxy(base)`). svelte@5.56.3 verified.
    let src =
        "<script>\n\tlet base = 5;\n\tbase = 6;\n\tlet x = $state(base);\n</script>\n<p>hi</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "x"),
        Some(StateLowering::BareProxy),
        "a reassigned identifier intermediate blocks the proxy follow → proxiable"
    );
}

#[test]
fn reassigned_never_read_primitive_is_state_signal() {
    let alloc = Allocator::default();
    // `n` is reassigned in a handler, never read anywhere. svelte@5.56.3 emits
    // `let n = $.state(0)`.
    let src =
        "<script>\n\tlet n = $state(0);\n</script>\n<button onclick={() => { n = 5; }}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "n"),
        Some(StateLowering::StateSignal),
        "a reassigned-never-read primitive $state is a signal"
    );
    assert_ne!(
        state_lowering(&ir, "n"),
        Some(StateLowering::PlainLet),
        "a reassignment is a write — it must NOT collapse to PlainLet"
    );
}

#[test]
fn function_body_read_only_unwritten_is_plain_let() {
    let alloc = Allocator::default();
    // `n` is read only inside a non-reactive function body, never written.
    // svelte@5.56.3 emits `let n = 0` (PlainLet) — a read does not make a $state
    // reactive; only a write does.
    let src = "<script>\n\tlet n = $state(0);\n\tfunction log(){ console.log(n); }\n</script>\n<button onclick={log}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "n"),
        Some(StateLowering::PlainLet),
        "a never-written $state read only in a function body lowers to a plain let"
    );
}

#[test]
fn state_written_only_inside_a_computed_member_key_is_reactive() {
    let alloc = Allocator::default();
    // A `$state` reassigned ONLY inside a computed-member KEY (`arr[count = 5] = 9`)
    // must be detected as reassigned → a signal. Confirmed vs svelte@5.56.3:
    // `count` lowers to `$.state(0)` with `$.set(count, 5)` in the key. The
    // pre-fix script use-collector never recursed into the computed key, so it
    // missed the write and (wrongly) classified `count` PlainLet.
    let src = "<script>\n\tlet count = $state(0);\n\tlet arr = $state([1]);\n\tfunction f(){ arr[count = 5] = 9; }\n</script>\n<button onclick={f}>{count}</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "count"),
        Some(StateLowering::StateSignal),
        "a `$state` reassigned inside a computed-member key is a reactive signal"
    );
    assert_ne!(
        state_lowering(&ir, "count"),
        Some(StateLowering::PlainLet),
        "the computed-key write must NOT be missed (leaving `count` a plain let)"
    );
}

#[test]
fn derived_or_effect_read_of_unwritten_state_is_plain_let() {
    let alloc = Allocator::default();
    // A read inside $derived / $effect of a never-written binding still collapses
    // to a plain `let` (svelte@5.56.3).
    let src_d = "<script>\n\tlet n = $state(0);\n\tlet d = $derived(n + 1);\n</script>\n<p>{d}</p>";
    let ir = lower(src_d, &alloc);
    assert_eq!(
        state_lowering(&ir, "n"),
        Some(StateLowering::PlainLet),
        "an unwritten $state read in $derived is a plain let"
    );

    let src_e =
        "<script>\n\tlet n = $state(0);\n\t$effect(() => console.log(n));\n</script>\n<p>x</p>";
    let ir = lower(src_e, &alloc);
    assert_eq!(
        state_lowering(&ir, "n"),
        Some(StateLowering::PlainLet),
        "an unwritten $state read in $effect is a plain let"
    );
}

// ---------------------------------------------------------------------------
// U1 — the binding table classifies the OTHER reactive runes, not just $state:
// `$derived`/`$derived.by` → Derived, a `$props()` destructure name → Prop, a
// `$bindable()` default → BindableProp, a `$props()` rest → Prop, and a whole-
// object `$props()` → Prop. The bindings enter the scope graph + binding table so
// a template read resolves SCOPE-AWARELY to the right kind (a shadowing local of
// the same name does NOT resolve to the rune binding). This is CLASSIFICATION +
// resolution only — the read-rewrite emission ($.get / $.prop) is the client/SSR backend's concern.
// Kinds confirmed against svelte@5.56.3: a $derived read is a $.get signal; a
// $props destructure is $.prop / $$props.x; a $bindable is the $.prop bindable.
// ---------------------------------------------------------------------------

mod rune_binding_classification {
    use super::*;

    #[test]
    fn props_destructure_names_classify_as_prop() {
        let alloc = Allocator::default();
        // The corpus `runes/props.svelte` shape: `let { name = 'world', count = 0 }
        // = $props();`. Both destructured names are Prop bindings (svelte@5.56.3
        // lowers each to `$.prop($$props, '<name>', 3, <default>)`).
        let src = "<script>\n\tlet { name = 'world', count = 0 } = $props();\n</script>\n<p>Hello {name} ({count})</p>";
        let ir = lower(src, &alloc);
        assert_eq!(
            root_binding_kind(&ir, "name"),
            Some(BindingRuntimeKind::Prop),
            "a `$props()` destructured name classifies as Prop"
        );
        assert_eq!(
            root_binding_kind(&ir, "count"),
            Some(BindingRuntimeKind::Prop),
            "a second `$props()` destructured name classifies as Prop"
        );
        // Negative: a Prop is NOT mis-classified as a plain local or a state signal.
        assert_ne!(
            root_binding_kind(&ir, "name"),
            Some(BindingRuntimeKind::PlainLocal),
            "a `$props()` name must NOT be left an unclassified plain local"
        );
    }

    #[test]
    fn derived_binding_classifies_as_derived() {
        let alloc = Allocator::default();
        // The corpus `runes/derived_and_effect.svelte` shape: `let doubled =
        // $derived(count * 2);`. `doubled` is a Derived memo (svelte@5.56.3 lowers
        // it to `$.derived(() => …)` read via `$.get`).
        let src = "<script>\n\tlet count = $state(0);\n\tlet doubled = $derived(count * 2);\n\t$effect(() => { console.log(doubled); });\n</script>\n<button onclick={() => count++}>{count} / {doubled}</button>";
        let ir = lower(src, &alloc);
        assert_eq!(
            root_binding_kind(&ir, "doubled"),
            Some(BindingRuntimeKind::Derived),
            "a `$derived(...)` binding classifies as Derived"
        );
        // Negative: `doubled` must not be left unresolved (the pre-fix gap — the
        // Derived variant existed but no scan produced it).
        assert!(
            root_binding_kind(&ir, "doubled").is_some(),
            "a `$derived` binding must enter the binding table (not stay unresolved)"
        );
    }

    #[test]
    fn derived_by_binding_classifies_as_derived() {
        let alloc = Allocator::default();
        // `$derived.by(() => …)` lowers identically to `$.derived(...)`.
        let src = "<script>\n\tlet a = $state(1);\n\tlet d = $derived.by(() => a * 2);\n</script>\n<p>{d}</p>";
        let ir = lower(src, &alloc);
        assert_eq!(
            root_binding_kind(&ir, "d"),
            Some(BindingRuntimeKind::Derived),
            "a `$derived.by(fn)` binding classifies as Derived"
        );
    }

    #[test]
    fn bindable_prop_classifies_as_bindable_prop() {
        let alloc = Allocator::default();
        // `let { value = $bindable(0) } = $props();` — `value` is a BindableProp
        // (svelte@5.56.3 lowers it to the bindable `$.prop($$props, 'value', 15,
        // 0)` — the `15` flag = bindable), distinct from a plain Prop.
        let src = "<script>\n\tlet { value = $bindable(0), other } = $props();\n</script>\n<input bind:value /><p>{other}</p>";
        let ir = lower(src, &alloc);
        assert_eq!(
            root_binding_kind(&ir, "value"),
            Some(BindingRuntimeKind::BindableProp),
            "a `$bindable()` default classifies the name as BindableProp"
        );
        // The sibling plain destructure name stays a (non-bindable) Prop.
        assert_eq!(
            root_binding_kind(&ir, "other"),
            Some(BindingRuntimeKind::Prop),
            "a plain sibling destructure name stays Prop (not BindableProp)"
        );
        // Negative: a BindableProp is DISTINCT from a plain Prop.
        assert_ne!(
            root_binding_kind(&ir, "value"),
            Some(BindingRuntimeKind::Prop),
            "a `$bindable` prop must be classified BindableProp, not a plain Prop"
        );
    }

    #[test]
    fn props_rest_and_whole_object_classify_as_prop() {
        let alloc = Allocator::default();
        // A `$props()` REST (`...rest`) and a WHOLE-object `let p = $props()` are
        // both Prop bindings (svelte@5.56.3 lowers both via `$.rest_props`).
        let src_rest = "<script>\n\tlet { a, ...rest } = $props();\n</script>\n<p>{a}</p>";
        let ir = lower(src_rest, &alloc);
        assert_eq!(
            root_binding_kind(&ir, "rest"),
            Some(BindingRuntimeKind::Prop),
            "a `$props()` rest binding classifies as Prop"
        );
        assert_eq!(
            root_binding_kind(&ir, "a"),
            Some(BindingRuntimeKind::Prop),
            "a `$props()` destructure name alongside a rest classifies as Prop"
        );

        let src_whole = "<script>\n\tlet p = $props();\n</script>\n<p>{p.x}</p>";
        let ir = lower(src_whole, &alloc);
        assert_eq!(
            root_binding_kind(&ir, "p"),
            Some(BindingRuntimeKind::Prop),
            "a whole-object `let p = $props()` classifies as Prop"
        );
    }

    #[test]
    fn shadowing_local_does_not_resolve_to_the_rune_binding() {
        let alloc = Allocator::default();
        // An outer `$derived doubled` shadowed by an each-binding `doubled`: the
        // template `{doubled}` inside the each body resolves to the SHADOWING each
        // binding (an EachSignal), NOT the outer Derived. A scope-blind classifier
        // would wrongly report Derived for the inner read.
        let src = "<script>\n\tlet count = $state(0);\n\tlet doubled = $derived([1, 2]);\n</script>\n{#each doubled as doubled}<p>{doubled}</p>{/each}";
        let ir = lower(src, &alloc);
        // The OUTER `doubled` (root scope) is still classified Derived.
        assert_eq!(
            root_binding_kind(&ir, "doubled"),
            Some(BindingRuntimeKind::Derived),
            "the outer `$derived doubled` is classified Derived at root scope"
        );
        // The INNER `{doubled}` read (inside the each body) resolves to the each
        // binding, not the outer Derived. Find the each-body scope and resolve
        // `doubled` there.
        let each_body_scope = find_each_body_scope(&ir).expect("the each block has a body scope");
        let inner_kind =
            ir.analysis
                .bindings
                .resolve_kind(&ir.analysis.scopes, each_body_scope, "doubled");
        assert_eq!(
            inner_kind,
            Some(BindingRuntimeKind::EachSignal),
            "a shadowing each binding resolves to EachSignal, NOT the outer Derived (got {inner_kind:?})"
        );
        assert_ne!(
            inner_kind,
            Some(BindingRuntimeKind::Derived),
            "the inner shadowed read must NOT resolve to the outer Derived rune binding"
        );
    }

    /// The body scope of the first `{#each}` block in the IR (for the shadowing
    /// resolution test).
    fn find_each_body_scope(
        ir: &super::super::ir::SvelteRuntimeIr,
    ) -> Option<super::super::expr::ScopeId> {
        ir.nodes.iter().find_map(|n| match n {
            IrNode::Block(BlockIr::Each { body, .. }) => Some(ir.template_scope(*body).scope),
            _ => None,
        })
    }
}

// ---------------------------------------------------------------------------
// F1 — scope-aware reactivity classification. An outer `$state count` that is
// referenced ONLY through a shadowing local (`{#each … as count}` / `{@const
// count}`) must NOT be marked reactive — it stays PlainLet because the outer
// binding is never actually read OR written. FAILS against a scope-blind
// flat-set of template names that counts a shadowing reference as the outer one.
// ---------------------------------------------------------------------------

#[test]
fn outer_state_shadowed_by_each_binding_is_not_reactive() {
    let alloc = Allocator::default();
    // The outer `count` $state is never written, and the only `count` reference
    // in the template (`{count}`) resolves to the SHADOWING each binding — so the
    // outer `count` is never read either. It must classify as PlainLet.
    let src = "<script>\n\tlet count = $state(0);\n\tlet items = $state([1, 2]);\n</script>\n{#each items as count}<p>{count}</p>{/each}";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "count"),
        Some(StateLowering::PlainLet),
        "an outer $state referenced only via a shadowing each binding is NOT reactive"
    );
}

#[test]
fn outer_state_shadowed_by_at_const_is_not_reactive() {
    let alloc = Allocator::default();
    // `{@const count = …}` shadows the outer `count` $state; the `{count}` read
    // inside resolves to the const local, not the outer signal. The outer state
    // is never written and never (un-shadowed) read → PlainLet.
    let src = "<script>\n\tlet count = $state(0);\n\tlet items = $state([{ q: 1 }]);\n</script>\n{#each items as item}{@const count = item.q}<p>{count}</p>{/each}";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "count"),
        Some(StateLowering::PlainLet),
        "an outer $state referenced only via a shadowing at-const is NOT reactive"
    );
}

#[test]
fn outer_state_read_unshadowed_in_template_with_write_is_signal() {
    let alloc = Allocator::default();
    // Control: the SAME outer `count`, written in a handler and read UN-shadowed
    // in the template, IS reactive — proving the shadow tests above discriminate
    // on the shadow, not on a blanket "never reactive".
    let src = "<script>\n\tlet count = $state(0);\n</script>\n<button onclick={() => count++}>{count}</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "count"),
        Some(StateLowering::StateSignal),
        "an un-shadowed, written outer $state is a signal"
    );
}

// ---------------------------------------------------------------------------
// F2 — the script use-collector must model nested LOCALS (let/const, catch
// params, for-loop bindings, function declarations), not just function params.
// A nested local of the same name as an outer $state must NOT attribute its
// writes to the outer binding. FAILS against the param-only scope stack.
// ---------------------------------------------------------------------------

#[test]
fn nested_let_shadow_does_not_attribute_write_to_outer_state() {
    let alloc = Allocator::default();
    // The outer `count` $state is never written; a nested `let count` inside a
    // handler is reassigned. The param-only stack would attribute `count++` to
    // the outer signal (→ StateSignal); the real lexical stack must NOT (the
    // outer stays PlainLet — never written, never read un-shadowed).
    let src = "<script>\n\tlet count = $state(0);\n</script>\n<button onclick={() => { let count = 0; count++; }}>x</button>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "count"),
        Some(StateLowering::PlainLet),
        "a nested `let count` reassignment must NOT mark the outer $state reassigned"
    );
}

#[test]
fn nested_let_shadow_in_script_function_does_not_mark_outer_reassigned() {
    let alloc = Allocator::default();
    // Same, in a SCRIPT function body (the ScriptUseCollector path). A nested
    // `let box` reassignment must not flip the outer object $state to StateProxy.
    // The outer object $state init is PROXIABLE, so with no (un-shadowed) reassign
    // it lowers to BareProxy (`$.proxy({a:1})`) — verified against svelte@5.56.3.
    // The discriminating fact is that the shadowed write does NOT promote it to
    // StateProxy (`$.state($.proxy(...))`).
    let src = "<script>\n\tlet box = $state({ a: 1 });\n\tfunction f(){ let box; box = { a: 2 }; }\n</script>\n<p>static</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "box"),
        Some(StateLowering::BareProxy),
        "a never-(un-shadowed-)reassigned object $state is a bare proxy"
    );
    assert_ne!(
        state_lowering(&ir, "box"),
        Some(StateLowering::StateProxy),
        "a nested-let `box` reassignment in a function body must NOT mark the outer $state reassigned"
    );
}

// ---------------------------------------------------------------------------
// H2 — module-script `$state` is modeled and resolvable from the template.
//
// `<script module>` `$state` declarations were absent from the binding table, so
// a template read of a module binding could not resolve. Official: a module
// `$state` reassigned in a module function is a signal the template reads. An
// instance binding shadows a same-named module binding. Module-script bindings
// live in a MODULE scope that is the PARENT of the template root scope, so an
// instance / template binding of the same name shadows the module one.
// ---------------------------------------------------------------------------

#[test]
fn module_script_state_classifies_and_resolves_from_template() {
    let alloc = Allocator::default();
    // A module `$state` reassigned in a module-script function is a signal; the
    // template `{x}` read must resolve to that module binding.
    let src = "<script module>\n\tlet x = $state(0);\n\tfunction f(){ x = 1; }\n</script>\n{x}";
    let ir = lower(src, &alloc);
    // The module binding classifies as a written-primitive signal (NOT PlainLet).
    assert_eq!(
        state_lowering(&ir, "x"),
        Some(StateLowering::StateSignal),
        "a written module-script $state must classify as a signal"
    );
    // The template root scope resolves `x` (to the module binding) — a read that
    // could not resolve before the module bindings were modeled.
    let root_scope = ir.root_scope().scope;
    let resolved = ir
        .analysis
        .scopes
        .resolve(&ir.analysis.bindings, root_scope, "x");
    assert!(
        resolved.is_some(),
        "the template root scope must resolve the module-script binding `x`"
    );
    assert_eq!(
        ir.analysis.bindings.get(resolved.unwrap()).kind,
        BindingRuntimeKind::StateSignal { raw: false },
        "the resolved module `x` is the written-primitive signal"
    );
}

#[test]
fn instance_binding_shadows_same_name_module_binding() {
    let alloc = Allocator::default();
    // Both a module `x` (plain, never reassigned at module scope) and an instance
    // `x` (reassigned via a template handler → signal) exist. The template `{x}`
    // resolves to the INSTANCE one (an instance binding shadows the module one).
    let src = "<script module>\n\tlet x = $state(0);\n</script>\n<script>\n\tlet x = $state(5);\n</script>\n<button onclick={() => x++}>{x}</button>";
    let ir = lower(src, &alloc);
    let root_scope = ir.root_scope().scope;
    let resolved = ir
        .analysis
        .scopes
        .resolve(&ir.analysis.bindings, root_scope, "x")
        .expect("x resolves at the template root scope");
    let resolved_info = ir.analysis.bindings.get(resolved);
    // The instance `x` is reassigned (`x++`) → a signal. The module `x` is never
    // reassigned → a plain let. Resolution must hit the INSTANCE signal.
    assert_eq!(
        resolved_info.kind,
        BindingRuntimeKind::StateSignal { raw: false },
        "the template read resolves to the INSTANCE `x` (a signal), not the module `x` (a plain let)"
    );
    // Negative: the module `x` binding (a distinct row) is the plain-let one — so
    // the shadowing is real (two distinct bindings, not one collapsed binding).
    let module_x = ir
        .analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == "x" && matches!(b.kind, BindingRuntimeKind::PlainLocal))
        .expect("the module-scope plain `x` binding exists distinctly");
    assert_ne!(
        module_x.kind, resolved_info.kind,
        "the module `x` and the instance `x` are distinct bindings (shadowing, not collapse)"
    );
}

#[test]
fn catch_param_shadow_does_not_attribute_write_to_outer_state() {
    let alloc = Allocator::default();
    // A `catch (n)` param shadows the outer `n` $state; reassigning the caught
    // `n` must not mark the outer signal reassigned.
    let src = "<script>\n\tlet n = $state(0);\n\tfunction f(){ try {} catch (n) { n = 5; } }\n</script>\n<p>static</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "n"),
        Some(StateLowering::PlainLet),
        "a catch-param `n` reassignment must NOT mark the outer $state reassigned"
    );
}

#[test]
fn for_loop_binding_shadow_does_not_attribute_write_to_outer_state() {
    let alloc = Allocator::default();
    // A `for (let i …)` binding shadows the outer `i` $state; mutating the loop
    // `i` must not mark the outer signal reassigned.
    let src = "<script>\n\tlet i = $state(0);\n\tfunction f(){ for (let i = 0; i < 3; i++) {} }\n</script>\n<p>static</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "i"),
        Some(StateLowering::PlainLet),
        "a for-loop `let i` mutation must NOT mark the outer $state reassigned"
    );
}

// ---------------------------------------------------------------------------
// H3 — the `should_proxy` one-hop reassign-follow is SCOPE-AWARE.
//
// The proxy follow `let base=5; let x=$state(base)` resolves `base` to its
// top-level declarator and uses ITS proxiability (a literal → non-proxiable, so
// `x` is a bare signal, no proxy) — UNLESS the TOP-LEVEL `base` is reassigned,
// which blocks the follow (then `x` stays proxiable → StateProxy). A reassignment
// of an inner SHADOWED `base` must NOT block the follow. The pre-fix flat
// whole-program reassign scan wrongly counted the inner write.
// ---------------------------------------------------------------------------

#[test]
fn proxy_follow_ignores_inner_shadowed_reassignment() {
    let alloc = Allocator::default();
    // `x` is reassigned (via `g`) → a signal. The proxy follow resolves `base` to
    // the top-level `let base = 5` (a literal → non-proxiable); the inner shadowed
    // `let base = 6; base = 7` reassigns the INNER binding only, so the follow is
    // NOT blocked → `x` is a NON-proxiable signal (`StateSignal`, no proxy).
    // official svelte@5.56.3: `let x = $.state(base)`.
    let src = "<script>\n\tlet base = 5;\n\tfunction f(){ let base = 6; base = 7; }\n\tlet x = $state(base);\n\tfunction g(){ x = base; }\n</script>\n<p>{x}</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "x"),
        Some(StateLowering::StateSignal),
        "an inner shadowed `base` reassignment must NOT block the proxy follow — `x` stays a bare signal"
    );
    // Negative: the scope-blind scan would have BLOCKED the follow (seeing the
    // inner `base = 7`), defaulting `x` to proxiable → StateProxy. Assert it is NOT.
    assert_ne!(
        state_lowering(&ir, "x"),
        Some(StateLowering::StateProxy),
        "a shadowed inner reassignment must not promote `x` to a proxied signal"
    );
}

#[test]
fn proxy_follow_blocked_by_toplevel_reassignment() {
    let alloc = Allocator::default();
    // Contrast: the TOP-LEVEL `base` is reassigned (via `h`), which BLOCKS the
    // follow → `x` stays proxiable → `StateProxy` (`$.state($.proxy(base))`).
    // official svelte@5.56.3: `let x = $.state($.proxy(base))`.
    let src = "<script>\n\tlet base = 5;\n\tfunction h(){ base = 7; }\n\tlet x = $state(base);\n\tfunction g(){ x = base; }\n</script>\n<p>{x}</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        state_lowering(&ir, "x"),
        Some(StateLowering::StateProxy),
        "a top-level `base` reassignment blocks the proxy follow — `x` stays proxiable (StateProxy)"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — a bare-proxy read is PLAIN member access, not a signal read.
// Discriminates against: a proxy-blind classifier that treats every $state as a
// signal (which would classify `box` as StateSignal / a `$.get` candidate).
// ---------------------------------------------------------------------------

#[test]
fn bare_proxy_binding_is_not_a_signal_read() {
    let alloc = Allocator::default();
    let src = "<script>\n\tlet box = $state({ a: 1, b: 2 });\n\tfunction bump(){ box.a += 1; }\n</script>\n<button>{box.a} {box.b}</button>";
    let ir = lower(src, &alloc);
    let kind = root_binding_kind(&ir, "box").expect("box is bound at root scope");
    assert_eq!(
        kind,
        BindingRuntimeKind::BareProxy,
        "a deep-mutated-only object $state is a bare proxy"
    );
    // Negative: it is NOT a signal, so it is not a `$.get` candidate.
    assert!(
        !matches!(kind, BindingRuntimeKind::StateSignal { .. }),
        "a bare proxy must NOT be classified as a $.state signal"
    );
    assert_ne!(
        state_lowering(&ir, "box"),
        Some(StateLowering::StateSignal),
        "a bare proxy must not lower to a plain signal"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — each item + await then-binding resolve as SIGNAL bindings.
// Discriminates against: treating each/await bindings as inert plain locals.
// ---------------------------------------------------------------------------

#[test]
fn each_item_and_await_then_bindings_are_signals() {
    let alloc = Allocator::default();

    let src_each = "<script>\n\tlet items = $state([1, 2]);\n</script>\n{#each items as item}<li>{item}</li>{/each}";
    let ir = lower(src_each, &alloc);
    // The item binding lives in the each body scope; find it.
    let each_item = ir
        .analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == "item")
        .expect("each item binding exists");
    assert_eq!(
        each_item.kind,
        BindingRuntimeKind::EachSignal,
        "an each item binding is a signal"
    );

    let src_await = "<script>\n\tlet p = fetch('/');\n</script>\n{#await p then value}<span>{value}</span>{/await}";
    let ir = lower(src_await, &alloc);
    let then_binding = ir
        .analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == "value")
        .expect("await then binding exists");
    assert_eq!(
        then_binding.kind,
        BindingRuntimeKind::AwaitSignal,
        "an await then binding is a signal"
    );
}

// ---------------------------------------------------------------------------
// Test 4 — shadowing (five cases). Each FAILS a scope-blind classifier and
// PASSES the scope-aware one: an inner local of the same name as a rune signal
// must NOT resolve as the signal.
// ---------------------------------------------------------------------------

#[test]
fn each_as_shadows_outer_signal() {
    let alloc = Allocator::default();
    // `name` is a $state signal; the each binding `name` shadows it inside the
    // each body. Resolving `name` in the each-body scope must yield the each
    // signal, NOT the outer state signal.
    let src =
        "<script>\n\tlet name = $state('a');\n</script>\n{#each [1] as name}<li>{name}</li>{/each}";
    let ir = lower(src, &alloc);
    // The each body scope is the inner template scope.
    let each_block = find_each_block(&ir);
    let body_scope = ir.template_scope(each_block_body(&ir, each_block)).scope;
    let kind = ir
        .analysis
        .bindings
        .resolve_kind(&ir.analysis.scopes, body_scope, "name")
        .expect("name resolves in the each body scope");
    assert_eq!(
        kind,
        BindingRuntimeKind::EachSignal,
        "inside the each body, `name` is the each binding, not the outer signal"
    );
    assert!(
        !matches!(kind, BindingRuntimeKind::StateSignal { .. }),
        "the shadowing each binding must NOT resolve as the outer $state signal"
    );
}

#[test]
fn snippet_param_shadows_outer_signal() {
    let alloc = Allocator::default();
    let src = "<script>\n\tlet row = $state('x');\n</script>\n{#snippet item(row)}<span>{row}</span>{/snippet}";
    let ir = lower(src, &alloc);
    let snippet_body = find_snippet_body(&ir);
    let body_scope = ir.template_scope(snippet_body).scope;
    let kind = ir
        .analysis
        .bindings
        .resolve_kind(&ir.analysis.scopes, body_scope, "row")
        .expect("row resolves in the snippet body scope");
    assert_eq!(
        kind,
        BindingRuntimeKind::SnippetParam,
        "inside the snippet body, `row` is the snippet param, not the outer signal"
    );
    assert!(!matches!(kind, BindingRuntimeKind::StateSignal { .. }));
}

#[test]
fn await_then_shadows_outer_signal() {
    let alloc = Allocator::default();
    let src = "<script>\n\tlet value = $state(0);\n\tlet p = fetch('/');\n</script>\n{#await p then value}<span>{value}</span>{/await}";
    let ir = lower(src, &alloc);
    let then_scope = ir
        .analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == "value" && b.kind == BindingRuntimeKind::AwaitSignal)
        .map(|b| b.scope)
        .expect("await then binding scope exists");
    let kind = ir
        .analysis
        .bindings
        .resolve_kind(&ir.analysis.scopes, then_scope, "value")
        .expect("value resolves in the then scope");
    assert_eq!(
        kind,
        BindingRuntimeKind::AwaitSignal,
        "inside the then branch, `value` is the await binding, not the outer signal"
    );
    assert!(!matches!(kind, BindingRuntimeKind::StateSignal { .. }));
}

#[test]
fn at_const_shadows_outer_signal() {
    let alloc = Allocator::default();
    // `{@const total}` introduces a derived local `total` inside the each body;
    // an outer `total` signal must be shadowed by it.
    let src = "<script>\n\tlet total = $state(0);\n\tlet items = $state([{q:1}]);\n</script>\n{#each items as item}{@const total = item.q}<li>{total}</li>{/each}";
    let ir = lower(src, &alloc);
    // The `total` introduced by {@const} is a LegacyConstDerived binding.
    let const_total = ir
        .analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == "total" && b.kind == BindingRuntimeKind::LegacyConstDerived)
        .expect("at-const total binding exists");
    let kind = ir
        .analysis
        .bindings
        .resolve_kind(&ir.analysis.scopes, const_total.scope, "total")
        .expect("total resolves in the at-const scope");
    assert_eq!(
        kind,
        BindingRuntimeKind::LegacyConstDerived,
        "the at-const `total` shadows the outer $state signal"
    );
    assert!(!matches!(kind, BindingRuntimeKind::StateSignal { .. }));
}

#[test]
fn nested_function_param_shadows_outer_signal() {
    let alloc = Allocator::default();
    // A nested function param `name` shadows the outer `name` signal inside the
    // event handler. The handler expression's free references must EXCLUDE the
    // shadowed `name` (it is a local of the arrow), so `name` is not treated as
    // the outer signal there.
    let src = "<script>\n\tlet name = $state('a');\n</script>\n<button onclick={(name) => console.log(name)}>{name}</button>";
    let ir = lower(src, &alloc);
    // The handler expression's analyzed references must NOT include the local
    // `name` (it is shadowed by the arrow param), while the interpolation's
    // reference DOES include the outer `name`.
    let handler_refs = handler_references(&ir);
    assert!(
        !handler_refs.iter().any(|r| r == "name"),
        "the nested-arrow param `name` shadows the outer signal — it is NOT a free reference in the handler"
    );
    let interp_refs = interpolation_references(&ir);
    assert!(
        interp_refs.iter().any(|r| r == "name"),
        "the interpolation references the outer signal (not shadowed)"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — declaration tags {const}/{let} are INERT (TemplateDeclLocal),
// DISTINCT from {@const} (LegacyConstDerived).
// ---------------------------------------------------------------------------

#[test]
fn declaration_tag_is_inert_distinct_from_at_const() {
    let alloc = Allocator::default();

    // `{@const}` → a derived binding.
    let src_at = "<script>\n\tlet items = $state([{q:1}]);\n</script>\n{#each items as item}{@const total = item.q}<li>{total}</li>{/each}";
    let ir_at = lower(src_at, &alloc);
    let at_const = ir_at
        .analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == "total")
        .expect("at-const total binding exists");
    assert_eq!(
        at_const.kind,
        BindingRuntimeKind::LegacyConstDerived,
        "at-const introduces a derived local"
    );

    // `{const}` declaration tag → an inert template-decl local.
    let src_decl = "<script>\n\tlet items = $state([{q:1}]);\n</script>\n{#each items as item}{const total = item.q}<li>{total}</li>{/each}";
    let ir_decl = lower(src_decl, &alloc);
    let decl_const = ir_decl
        .analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == "total")
        .expect("const-tag total binding exists");
    assert_eq!(
        decl_const.kind,
        BindingRuntimeKind::TemplateDeclLocal,
        "a const declaration tag introduces an INERT local"
    );

    // Negative: the two kinds are DISTINCT.
    assert_ne!(
        at_const.kind, decl_const.kind,
        "at-const (derived) and the const declaration tag (inert decl-local) must lower to different binding kinds"
    );
}

// ---------------------------------------------------------------------------
// {let x} no-init — a no-initializer declaration tag must parse. The pre-fix
// code wrapped every declaration-tag inner text with `const <inner>;`, so
// `{let x}` (no init) became invalid JS (`const x;`) and failed to parse,
// dropping the binding. Wrapping a `{let …}` with `let` parses it.
// ---------------------------------------------------------------------------

#[test]
fn let_declaration_tag_no_initializer_parses_to_decl_local() {
    let alloc = Allocator::default();
    // `{let x}` with NO initializer — must lower to a TemplateDeclLocal binding,
    // not fail to parse. The lowering succeeds (no diagnostic) and `x` is bound.
    let src = "<div>{let x}<p>{x}</p></div>";
    let parsed = parse_svelte(src);
    let ir = lower_parsed_svelte_to_ir(src, &parsed, &SvelteRuntimeOptions::default(), &alloc)
        .expect("a no-init `{let x}` declaration tag lowers without error");
    let binding = ir
        .analysis
        .bindings
        .all()
        .iter()
        .find(|b| b.name == "x")
        .expect("the `{let x}` declaration tag binds `x`");
    assert_eq!(
        binding.kind,
        BindingRuntimeKind::TemplateDeclLocal,
        "a `{{let x}}` (no init) is an inert template-decl local"
    );
    // Control: `{let x = 1}` (with init) still parses + binds.
    let src2 = "<div>{let x = 1}<p>{x}</p></div>";
    let parsed2 = parse_svelte(src2);
    let ir2 = lower_parsed_svelte_to_ir(src2, &parsed2, &SvelteRuntimeOptions::default(), &alloc)
        .expect("an init `{let x = 1}` lowers");
    assert!(
        ir2.analysis.bindings.all().iter().any(|b| b.name == "x"),
        "a `{{let x = 1}}` declaration tag binds `x`"
    );
}

// ---------------------------------------------------------------------------
// H5 — the client `ImportPlan` derives its flag set from the component mode: a
// LEGACY (non-runes) component carries the `svelte/internal/flags/legacy`
// side-effect import (`legacy_flag = true`); a runes component does not. (async /
// tracing flags stay false — their lowering is a downstream layer; no corpus
// fixture exercises them.)
// ---------------------------------------------------------------------------

#[test]
fn legacy_component_import_plan_sets_legacy_flag() {
    let alloc = Allocator::default();
    // A store-auto-subscription component uses NO runes → legacy mode → the
    // import plan carries `legacy_flag`. official svelte@5.56.3:
    // `import 'svelte/internal/flags/legacy'`.
    let src = "<script>\n\timport { writable } from 'svelte/store';\n\tconst count = writable(0);\n</script>\n<button>{$count}</button>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        topo.imports.legacy_flag,
        "a legacy (non-runes) component sets the legacy flag"
    );
    // The disclose-version + client namespace are always present; async / tracing
    // stay false (later-block features, no fixture).
    assert!(topo.imports.disclose_version, "disclose-version is present");
    assert!(!topo.imports.async_flag, "async flag stays false");
    assert!(!topo.imports.tracing_flag, "tracing flag stays false");
}

#[test]
fn runes_component_import_plan_clears_legacy_flag() {
    let alloc = Allocator::default();
    // A runes component (`$state`) → runes mode → NO legacy flag.
    let src = "<script>\n\tlet count = $state(0);\n\tfunction inc(){ count += 1; }\n</script>\n<button onclick={inc}>{count}</button>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        !topo.imports.legacy_flag,
        "a runes component must NOT set the legacy flag"
    );
    assert!(topo.imports.disclose_version, "disclose-version is present");
}

// ---------------------------------------------------------------------------
// The topology helper-recorder must apply the SAME host-attribute gate as the
// emitter (`compile_client`): for an invalid host shape (a `bind:checked` with no
// `type`, a `bind:group` with a valueless `type`), the emitter FAILS CLOSED, so the
// topology recorder must record NO bind helper — otherwise the structural oracle and
// the emitter would DISAGREE. RED before the fix (the recorder called
// `resolve_runtime_bind` directly, recording the helper for a host shape the emitter
// refuses); GREEN after routing the recorder through `host_attr_gate_passes`.
// ---------------------------------------------------------------------------

#[test]
fn topology_records_no_bind_helper_for_checked_without_type() {
    let alloc = Allocator::default();
    // `<input bind:checked>` with NO `type` attr: official ERRORs ("`bind:checked`
    // can only be used with `<input type="checkbox">`") and `compile_client` fails
    // closed, so the topology recorder must NOT record `BindChecked`.
    let src = "<script>let c = $state(false);</script>\n<input bind:checked={c} />\n";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        !topo.helpers.uses(SvelteHelper::BindChecked),
        "a refused `bind:checked` (no type) must record NO bind helper (the recorder \
         must apply the same host-attr gate as the emitter):\n{:?}",
        topo.helpers.helper_set()
    );
}

#[test]
fn topology_records_no_bind_helper_for_group_with_valueless_type() {
    let alloc = Allocator::default();
    // `<input type bind:group>` (VALUELESS type): official ERRORs ("'type' attribute
    // must be a static text value if input uses two-way binding") and
    // `compile_client` fails closed, so the topology recorder must NOT record
    // `BindGroup`.
    let src = "<script>let g = $state(\"\");</script>\n<input type bind:group={g} value=\"a\" />\n";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        !topo.helpers.uses(SvelteHelper::BindGroup),
        "a refused `bind:group` (valueless type) must record NO bind helper:\n{:?}",
        topo.helpers.helper_set()
    );
}

#[test]
fn topology_still_records_bind_helper_for_valid_group() {
    let alloc = Allocator::default();
    // POSITIVE control: a VALID `<input type="radio" bind:group>` (static type) is
    // accepted by the emitter, so the topology recorder must STILL record `BindGroup`
    // (the gate must not over-refuse). Verified against svelte@5.56.3.
    let src = "<script>let g = $state(\"\");</script>\n<input type=\"radio\" bind:group={g} value=\"a\" />\n";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        topo.helpers.uses(SvelteHelper::BindGroup),
        "a valid static-type `bind:group` must still record the bind helper:\n{:?}",
        topo.helpers.helper_set()
    );
}

// ---------------------------------------------------------------------------
// U4 — runtime mode inference honors an explicit `<svelte:options runes={…}>`
// override (Svelte's own forced-mode switch), shared with the IDE projector via
// the parser-owned `forced_runes_option`. `runes={true}` forces RUNES mode even with zero
// rune calls; `runes={false}` forces LEGACY mode even when a `$state` rune is
// present. Derived empirically against svelte@5.56.3 (a `runes={true}` component
// emits no `flags/legacy` import; a `runes={false}` + `$state` component emits
// it). FAILS against the inference that only reads rune USAGE.
// ---------------------------------------------------------------------------

#[test]
fn svelte_options_runes_true_forces_runes_mode_without_rune_calls() {
    let alloc = Allocator::default();
    // official svelte@5.56.3: `<svelte:options runes={true} />` + a plain `let x = 1`
    // (NO rune call) compiles in RUNES mode — no `svelte/internal/flags/legacy`
    // import. The runtime IR mode must be Runes.
    let src = "<svelte:options runes={true} />\n<script>let x = 1;</script>\n<div>{x}</div>";
    let ir = lower(src, &alloc);
    assert_eq!(
        ir.component.mode,
        super::ir::SvelteMode::Runes,
        "an explicit `<svelte:options runes={{true}}>` forces runes mode despite zero rune calls"
    );
    // Negative (the mode-derived flag): a runes component carries NO legacy flag.
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        !topo.imports.legacy_flag,
        "forced-runes mode must NOT set the legacy flag"
    );
}

#[test]
fn svelte_options_runes_false_forces_legacy_mode_with_state_rune() {
    let alloc = Allocator::default();
    // official svelte@5.56.3: `<svelte:options runes={false} />` + a `$state(0)` is
    // LEGACY (the `$state` is treated as a plain local) — it emits the
    // `svelte/internal/flags/legacy` import. The runtime IR mode must be Legacy
    // even though a rune NAME appears.
    let src =
        "<svelte:options runes={false} />\n<script>let x = $state(0);</script>\n<div>{x}</div>";
    let ir = lower(src, &alloc);
    assert_eq!(
        ir.component.mode,
        super::ir::SvelteMode::Legacy,
        "an explicit `<svelte:options runes={{false}}>` forces legacy mode even with a `$state` rune present"
    );
    // The mode-derived flag: a legacy component carries the legacy flag.
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        topo.imports.legacy_flag,
        "forced-legacy mode sets the legacy flag"
    );
}

#[test]
fn svelte_options_runes_shorthand_forces_runes_mode() {
    let alloc = Allocator::default();
    // The valueless `<svelte:options runes />` boolean shorthand is `runes={true}`.
    let src = "<svelte:options runes />\n<script>let x = 1;</script>\n<div>{x}</div>";
    let ir = lower(src, &alloc);
    assert_eq!(
        ir.component.mode,
        super::ir::SvelteMode::Runes,
        "the `<svelte:options runes>` boolean shorthand forces runes mode"
    );
}

#[test]
fn shadowing_function_param_named_rune_keeps_component_legacy() {
    // X5 — a function parameter named `$state` SHADOWS the rune: `$state` inside
    // `f`'s body resolves to the parameter, NOT the rune. svelte@5.56.3 keeps such
    // a component in LEGACY mode (EMPIRICALLY confirmed: the legacy flag import is
    // present). The detection must be SCOPE-AWARE — FAILS against the prior
    // any-identifier-named-`$state` detector that forced runes mode.
    let alloc = Allocator::default();
    let src = "<script>function f($state){ return $state; } let y = 1;</script>\n<p>{y}</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        ir.component.mode,
        super::ir::SvelteMode::Legacy,
        "a function-param `$state` is a shadowing local, not a rune — the component stays legacy"
    );
    // The mode-derived flag confirms it: a legacy component carries the legacy flag.
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        topo.imports.legacy_flag,
        "the shadowed-rune component is legacy → carries the legacy flag"
    );
}

#[test]
fn real_state_rune_call_makes_component_runes() {
    // X5 (positive control) — an actual `$state(0)` initializer (an UNRESOLVED
    // rune reference) makes the component RUNES (no legacy flag). This is the
    // discriminating pair partner: same `$state` name, but un-shadowed → runes.
    let alloc = Allocator::default();
    let src = "<script>let x = $state(0);</script>\n<p>{x}</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        ir.component.mode,
        super::ir::SvelteMode::Runes,
        "a real (un-shadowed) `$state(0)` call forces runes mode"
    );
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        !topo.imports.legacy_flag,
        "the real-rune component is runes → no legacy flag"
    );
}

#[test]
fn bare_rune_reference_without_call_still_forces_runes() {
    // X5 — the official detection counts ANY unresolved rune reference, not only a
    // CALL. A bare `$inspect` reference (un-shadowed) forces runes mode
    // (EMPIRICALLY confirmed: `$inspect(y)` is runes; a bare reference to a rune
    // name with no local binding is an unresolved reference → runes).
    let alloc = Allocator::default();
    let src = "<script>let y = 1; $inspect(y);</script>\n<p>{y}</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        ir.component.mode,
        super::ir::SvelteMode::Runes,
        "an unresolved `$inspect` reference forces runes mode"
    );
}

#[test]
fn module_script_rune_forces_runes_mode() {
    // X5 — a rune used in the MODULE `<script module>` forces runes mode even when
    // the instance script has none (EMPIRICALLY confirmed against svelte@5.56.3).
    let alloc = Allocator::default();
    let src = "<script module>export const c = $state(0);</script>\n<script>let y = 1;</script>\n<p>{y}</p>";
    let ir = lower(src, &alloc);
    assert_eq!(
        ir.component.mode,
        super::ir::SvelteMode::Runes,
        "a module-script rune forces the whole component into runes mode"
    );
}

// ---------------------------------------------------------------------------
// F7-complete — a MALFORMED instance / module `<script>` (a non-empty OXC error
// set, not just a panic) must NOT silently feed a partial AST into rune / mode /
// state analysis: it records a diagnostic and fails lowering. FAILS against the
// pre-fix `reparse_module` that checked only `parsed.panicked`.
// ---------------------------------------------------------------------------

#[test]
fn malformed_instance_script_records_diagnostic() {
    let alloc = Allocator::default();
    // A RECOVERABLE OXC parse error (`?? ` mixed with `||` without parens): OXC
    // produces a FULL AST but a non-empty error set with `panicked == false`. The
    // pre-fix `reparse_module` checked only `panicked`, so it would have ACCEPTED
    // this torn parse and fed it into state analysis; the fix rejects on
    // `parsed.errors` and records a diagnostic. This case is the one that
    // discriminates the fix (a `panicked == true` case the old code already
    // caught would not).
    let src = "<script>let x = a ?? b || c;</script><p>x</p>";
    let parsed = parse_svelte(src);
    let result = lower_parsed_svelte_to_ir(src, &parsed, &SvelteRuntimeOptions::default(), &alloc);
    let errors =
        result.expect_err("a malformed (recoverable-error) instance script must fail lowering");
    assert!(
        errors
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-runtime-script-parse"),
        "a malformed instance script records a script-parse diagnostic (got {:?})",
        errors.diagnostics
    );

    // Control: a WELL-FORMED instance script lowers without a script-parse
    // diagnostic (the gate discriminates on malformedness, not on every script).
    let ok_src = "<script>let x = $state(0);</script><p>{x}</p>";
    let ok_parsed = parse_svelte(ok_src);
    let ok =
        lower_parsed_svelte_to_ir(ok_src, &ok_parsed, &SvelteRuntimeOptions::default(), &alloc);
    assert!(
        ok.is_ok(),
        "a well-formed instance script lowers cleanly (got {ok:?})"
    );
}

#[test]
fn same_scope_redeclaration_refuses_via_scope_facts() {
    let alloc = Allocator::default();
    // A same-scope redeclaration (`const a = 1; const a = 2;`) is PARSE-valid but
    // fails SemanticBuilder scope analysis, so the component-scope facts REFUSE
    // (`svelte-runtime-scope-facts`) rather than fabricate an un-deconflicted
    // component name from partial facts. svelte@5.56.3 likewise rejects it
    // (`Identifier 'a' has already been declared`) — the refusal is oracle-aligned.
    let src = "<script>const a = 1; const a = 2;</script>\n<p>x</p>\n";
    let errors =
        lower_result(src, &alloc).expect_err("a same-scope redeclaration must fail lowering");
    assert!(
        errors
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-runtime-scope-facts"),
        "a same-scope redeclaration refuses via the scope-facts channel (got {:?})",
        errors.diagnostics
    );

    // Control: a well-formed instance script lowers cleanly (no scope-facts refusal).
    let ok = lower_result("<script>const a = 1;</script>\n<p>x</p>\n", &alloc);
    assert!(
        ok.is_ok(),
        "a well-formed script must not trip the scope-facts refusal (got {ok:?})"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — multi-root templates set the FragmentOne flag; a single-root template
// does not.
// ---------------------------------------------------------------------------

#[test]
fn fragment_flag_set_for_multi_root_absent_for_single() {
    let alloc = Allocator::default();

    let two_root = "<h1>a</h1>\n<p>b</p>";
    let ir = lower(two_root, &alloc);
    let plan = plan_static_templates(&ir, None);
    assert_eq!(
        first_fragment_flag(&plan.templates[0]),
        TemplateFlag::from_bits(TemplateFlag::FRAGMENT),
        "a 2-root template sets the fragment flag"
    );

    let three_root = "<h1>a</h1>\n<p>b</p>\n<span>c</span>";
    let ir = lower(three_root, &alloc);
    let plan = plan_static_templates(&ir, None);
    assert_eq!(
        first_fragment_flag(&plan.templates[0]),
        TemplateFlag::from_bits(TemplateFlag::FRAGMENT),
        "a 3-root template sets the same fragment flag"
    );

    let one_root = "<div><span>a</span><span>b</span></div>";
    let ir = lower(one_root, &alloc);
    let plan = plan_static_templates(&ir, None);
    assert_eq!(
        first_fragment_flag(&plan.templates[0]),
        None,
        "a single-root template has NO fragment flag"
    );
}

// ---------------------------------------------------------------------------
// Test 7 — a zero-element / block-only root plans a CommentAnchor, not FromHtml.
// ---------------------------------------------------------------------------

#[test]
fn block_only_root_plans_comment_anchor() {
    let alloc = Allocator::default();
    let src = "<script>\n\tlet show = $state(true);\n</script>\n{#if show}<p>x</p>{/if}";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    assert!(
        matches!(
            plan.templates[0],
            TemplateFactory::CommentAnchor {
                reason: AnchorReason::BlockOnlyRoot
            }
        ),
        "a block-only root plans a comment anchor, not from_html (got {:?})",
        plan.templates[0]
    );
    // Negative: it is NOT a from_html factory.
    assert!(
        !matches!(plan.templates[0], TemplateFactory::FromHtml { .. }),
        "a block-only root must NOT plan a from_html factory"
    );
}

// ---------------------------------------------------------------------------
// Test 8 — onclick registers `click` in DelegatedEvents; onfocus does NOT.
// ---------------------------------------------------------------------------

#[test]
fn onclick_is_delegated_onfocus_is_not() {
    let alloc = Allocator::default();
    let src = "<script>\n\tlet n = $state(0);\n</script>\n<button onclick={() => n++}>x</button>\n<input onfocus={() => {}} />";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert!(
        topo.delegated_events.contains("click"),
        "onclick registers `click` in the delegated set"
    );
    // Negative: onfocus is NOT delegated.
    assert!(
        !topo.delegated_events.contains("focus"),
        "onfocus must NOT enter the delegated set (it is a direct $.event listener)"
    );
    // The non-delegated event is recorded as `Event`, the delegated as `Delegated`.
    assert!(
        topo.helpers.uses(SvelteHelper::Delegated),
        "a delegated handler uses $.delegated"
    );
    assert!(
        topo.helpers.uses(SvelteHelper::Event),
        "a non-delegated handler uses $.event"
    );
}

// ---------------------------------------------------------------------------
// Test 8b — the `<svelte:element>` fold records exactly ONE `$.attribute_effect`.
// ---------------------------------------------------------------------------

#[test]
fn svelte_element_fold_plans_exactly_one_attribute_effect() {
    // A `<svelte:element>` whose attributes route to the FOLD emits exactly ONE
    // `$.attribute_effect` for the WHOLE co-located fold — the spread must NOT
    // double-record through the per-attribute recorder (the regular-element
    // spread arm) on top of the fold-route record. Official emits one call;
    // the planned count must equal it.
    let alloc = Allocator::default();
    let src = "<script>let tag = $state('div');\nlet { p } = $props();</script>\n<svelte:element this={tag} {...p}>x</svelte:element>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert_eq!(
        topo.helpers.count(SvelteHelper::AttributeEffect),
        1,
        "a spread <svelte:element> fold plans exactly one $.attribute_effect"
    );

    // The REGULAR-element spread control still records its single fold call.
    let src = "<script>let { p } = $props();</script>\n<div {...p}>x</div>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let topo = plan_client_topology(&ir, &plan, None);
    assert_eq!(
        topo.helpers.count(SvelteHelper::AttributeEffect),
        1,
        "a spread regular element plans exactly one $.attribute_effect"
    );
}

// ---------------------------------------------------------------------------
// Test 9 — IR-shape snapshot tests per block / tag / special element.
// ---------------------------------------------------------------------------

#[test]
fn block_and_tag_ir_shapes() {
    let alloc = Allocator::default();

    // {#if} → BlockIr::If with two branches (the if + the else).
    let ir = lower(
        "<script>let s = $state(true);</script>{#if s}<p>a</p>{:else}<p>b</p>{/if}",
        &alloc,
    );
    match find_block(&ir) {
        BlockIr::If { branches } => {
            assert_eq!(branches.len(), 2, "if/else has two branches");
            assert!(
                branches[0].condition.is_some(),
                "the if branch has a condition"
            );
            assert!(
                branches[1].condition.is_none(),
                "the else branch has no condition"
            );
        }
        other => panic!("expected an If block, got {other:?}"),
    }

    // {#each} keyed → BlockIr::Each with item + key.
    let ir = lower(
        "<script>let items = $state([{id:1}]);</script>{#each items as item (item.id)}<li>{item.id}</li>{/each}",
        &alloc,
    );
    match find_block(&ir) {
        BlockIr::Each { item, key, .. } => {
            assert!(item.is_some(), "the keyed each has an item binding");
            assert!(key.is_some(), "the keyed each has a key expression");
        }
        other => panic!("expected an Each block, got {other:?}"),
    }

    // {#each} unkeyed → BlockIr::Each with no key.
    let ir = lower(
        "<script>let items = $state([1]);</script>{#each items as item}<li>{item}</li>{/each}",
        &alloc,
    );
    match find_block(&ir) {
        BlockIr::Each { item, key, .. } => {
            assert!(item.is_some(), "the unkeyed each has an item binding");
            assert!(key.is_none(), "the unkeyed each has NO key expression");
        }
        other => panic!("expected an Each block, got {other:?}"),
    }

    // {#await} → BlockIr::Await.
    let ir = lower(
        "<script>let p = fetch('/');</script>{#await p}<span>loading</span>{:then v}<span>{v}</span>{:catch e}<span>{e}</span>{/await}",
        &alloc,
    );
    match find_block(&ir) {
        BlockIr::Await {
            then_binding,
            catch_binding,
            ..
        } => {
            assert!(then_binding.is_some(), "the await has a then binding");
            assert!(catch_binding.is_some(), "the await has a catch binding");
        }
        other => panic!("expected an Await block, got {other:?}"),
    }

    // {#key} → BlockIr::Key.
    let ir = lower(
        "<script>let k = $state(0);</script>{#key k}<span>{k}</span>{/key}",
        &alloc,
    );
    assert!(
        matches!(find_block(&ir), BlockIr::Key { .. }),
        "expected a Key block"
    );

    // {#snippet} → BlockIr::Snippet.
    let ir = lower("{#snippet row(n)}<li>{n}</li>{/snippet}", &alloc);
    assert!(
        matches!(find_block(&ir), BlockIr::Snippet { .. }),
        "expected a Snippet block"
    );

    // {@render} → TagIr::Render.
    let ir = lower("{@render row(1)}", &alloc);
    assert!(
        matches!(find_tag(&ir), TagIr::Render { .. }),
        "expected a Render tag"
    );

    // {@html} → TagIr::Html.
    let ir = lower(
        "<script>let h = $state('<b>x</b>');</script><div>{@html h}</div>",
        &alloc,
    );
    assert!(
        ir.nodes
            .iter()
            .any(|n| matches!(n, IrNode::Tag(TagIr::Html { .. }))),
        "expected an Html tag"
    );

    // {@debug} → TagIr::Debug.
    let ir = lower("<script>let x = $state(0);</script>{@debug x}", &alloc);
    assert!(
        matches!(find_tag(&ir), TagIr::Debug { .. }),
        "expected a Debug tag"
    );

    // {@attach} → TagIr::Attach.
    let ir = lower(
        "<script>let fn = () => {};</script><div>{@attach fn}</div>",
        &alloc,
    );
    assert!(
        ir.nodes
            .iter()
            .any(|n| matches!(n, IrNode::Tag(TagIr::Attach { .. }))),
        "expected an Attach tag"
    );

    // A representative special element: <svelte:head>.
    let ir = lower("<svelte:head><title>x</title></svelte:head>", &alloc);
    assert!(
        ir.nodes.iter().any(|n| matches!(
            n,
            IrNode::Special(s) if s.kind == SpecialKind::Head
        )),
        "expected a svelte:head special element"
    );
}

// ---------------------------------------------------------------------------
// F3 — a `{@const}` destructuring pattern declares one binding PER NAME, NOT one
// collapsed binding; the each/await PatternId arena is retained on the analysis
// (resolvable after lowering returns). FAILS against the collapse-onto-one and
// against the dropped pattern arena.
// ---------------------------------------------------------------------------

#[test]
fn at_const_destructure_declares_one_binding_per_name() {
    let alloc = Allocator::default();
    // `{@const {a, b} = obj}` must declare TWO distinct, resolvable bindings.
    let src = "<script>let items = $state([{a:1, b:2}]);</script>{#each items as item}{@const {a, b} = item}<p>{a}{b}</p>{/each}";
    let ir = lower(src, &alloc);

    // The LegacyConst node carries a PatternId resolving to TWO binding ids.
    let pattern = ir
        .nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Tag(TagIr::LegacyConst { pattern, .. }) => Some(*pattern),
            _ => None,
        })
        .expect("an at-const tag exists");
    let bindings = ir.pattern_bindings(pattern);
    assert_eq!(
        bindings.len(),
        2,
        "`{{@const {{a, b}}}}` declares two bindings, not one collapsed binding"
    );
    let names: Vec<&str> = bindings
        .iter()
        .map(|&b| ir.analysis.bindings.get(b).name.as_str())
        .collect();
    assert!(
        names.contains(&"a") && names.contains(&"b"),
        "both destructured names `a` and `b` are bound (got {names:?})"
    );
    // Negative: they are DISTINCT binding ids.
    assert_ne!(
        bindings[0], bindings[1],
        "the two destructured names must NOT collapse onto one BindingId"
    );
}

#[test]
fn each_pattern_id_resolves_after_lowering_returns() {
    let alloc = Allocator::default();
    // The each item PatternId must resolve through the retained pattern arena
    // AFTER `lower_parsed_svelte_to_ir` returns (the arena is owned by the
    // analysis, not dropped with the lowering context).
    let src =
        "<script>let items = $state([1, 2]);</script>{#each items as item}<li>{item}</li>{/each}";
    let ir = lower(src, &alloc);
    let item_pat = ir
        .nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Block(BlockIr::Each { item, .. }) => *item,
            _ => None,
        })
        .expect("an each item pattern exists");
    let bindings = ir.pattern_bindings(item_pat);
    assert_eq!(
        bindings.len(),
        1,
        "the each item pattern declares one binding"
    );
    assert_eq!(
        ir.analysis.bindings.get(bindings[0]).kind,
        BindingRuntimeKind::EachSignal,
        "the resolved each-item binding is an each signal"
    );
}

// ---------------------------------------------------------------------------
// F4 — a multi-branch `{#await p}<pending>{:then v}<then>{:catch e}<catch>` keeps
// THREE distinct bodies (pending / then / catch) with the right CONTENT — the
// parser-promoted inline then-binding must NOT collapse pending into the then
// body and discard the clause children. FAILS against the inline-then-first
// dispatch.
// ---------------------------------------------------------------------------

#[test]
fn multibranch_await_keeps_distinct_pending_then_catch_bodies() {
    let alloc = Allocator::default();
    let src = "<script>let p = fetch('/');</script>{#await p}<span>pending</span>{:then v}<b>{v}</b>{:catch e}<i>{e}</i>{/await}";
    let ir = lower(src, &alloc);
    let (pending, then_body, catch_body) = ir
        .nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Block(BlockIr::Await {
                pending,
                then_body,
                catch_body,
                ..
            }) => Some((*pending, *then_body, *catch_body)),
            _ => None,
        })
        .expect("an await block exists");

    let pending = pending.expect("the await has a pending body");
    let then_body = then_body.expect("the await has a then body");
    let catch_body = catch_body.expect("the await has a catch body");
    // All three are distinct template scopes.
    assert_ne!(pending, then_body, "pending and then are distinct bodies");
    assert_ne!(then_body, catch_body, "then and catch are distinct bodies");
    assert_ne!(pending, catch_body, "pending and catch are distinct bodies");

    // The BODY CONTENT (the element tag) of each branch is the right one.
    assert_eq!(
        scope_root_element_tag(&ir, pending),
        Some("span".to_string()),
        "the pending body is the <span> pending content"
    );
    assert_eq!(
        scope_root_element_tag(&ir, then_body),
        Some("b".to_string()),
        "the then body is the <b> then content, NOT the pending content"
    );
    assert_eq!(
        scope_root_element_tag(&ir, catch_body),
        Some("i".to_string()),
        "the catch body is the <i> catch content, NOT a duplicate of pending"
    );
}

#[test]
fn inline_await_then_has_then_body_no_pending() {
    let alloc = Allocator::default();
    // `{#await p then v}<b>{v}</b>{/await}` — the immediate children are the THEN
    // body; there is NO pending branch and NO catch branch.
    let src = "<script>let p = fetch('/');</script>{#await p then v}<b>{v}</b>{/await}";
    let ir = lower(src, &alloc);
    let (pending, then_body, catch_body) = await_bodies(&ir);
    assert!(pending.is_none(), "inline-then await has no pending branch");
    assert_eq!(
        scope_root_element_tag(&ir, then_body.expect("then body")),
        Some("b".to_string()),
        "the immediate children are the then body"
    );
    assert!(
        catch_body.is_none(),
        "inline-then await has no catch branch"
    );
}

#[test]
fn inline_await_catch_has_catch_body_no_pending() {
    let alloc = Allocator::default();
    // `{#await p catch e}<i>{e}</i>{/await}` — the immediate children are the CATCH
    // body; there is NO pending branch (the children must NOT be double-lowered as
    // both pending and catch) and NO then branch.
    let src = "<script>let p = fetch('/');</script>{#await p catch e}<i>{e}</i>{/await}";
    let ir = lower(src, &alloc);
    let (pending, then_body, catch_body) = await_bodies(&ir);
    assert!(
        pending.is_none(),
        "inline-catch await has NO pending branch (children are the catch body)"
    );
    assert!(then_body.is_none(), "inline-catch await has no then branch");
    assert_eq!(
        scope_root_element_tag(&ir, catch_body.expect("catch body")),
        Some("i".to_string()),
        "the immediate children are the catch body"
    );
}

// ---------------------------------------------------------------------------
// F7 — a malformed template expression records a diagnostic (it does NOT silently
// drop to no references). FAILS against the silent-None reparse.
// ---------------------------------------------------------------------------

#[test]
fn malformed_template_expression_records_diagnostic() {
    let alloc = Allocator::default();
    // `{1 +}` is not a valid expression — lowering must FAIL with a diagnostic.
    let src = "<p>{1 +}</p>";
    let parsed = parse_svelte(src);
    let result = lower_parsed_svelte_to_ir(src, &parsed, &SvelteRuntimeOptions::default(), &alloc);
    let errors = result.expect_err("a malformed expression must fail lowering");
    assert!(
        errors
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-runtime-expr-parse"),
        "a malformed expression surfaces an expr-parse diagnostic (got {:?})",
        errors.diagnostics
    );
    // Negative: a well-formed expression does NOT produce the diagnostic.
    let ok = lower_parsed_svelte_to_ir(
        "<p>{1 + 2}</p>",
        &parse_svelte("<p>{1 + 2}</p>"),
        &SvelteRuntimeOptions::default(),
        &alloc,
    );
    assert!(ok.is_ok(), "a well-formed expression lowers cleanly");
}

// ---------------------------------------------------------------------------
// F8 — a mixed attribute (`class="a {b}"`) splits into literal + expression runs;
// the literal `a ` is preserved and `{b}` is captured as a reference. FAILS
// against reparsing the whole value as one (invalid) expression that silently
// drops the refs.
// ---------------------------------------------------------------------------

#[test]
fn mixed_attribute_splits_literal_and_expression_runs() {
    use super::ir::{AttrIr, MixedAttrPart};
    let alloc = Allocator::default();
    let src = "<script>let b = $state('x');</script><div class=\"a {b}\">y</div>";
    let ir = lower(src, &alloc);
    let parts = ir
        .nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Element(el) => el.attrs.iter().find_map(|a| match a {
                AttrIr::Mixed { name, parts } if name == "class" => Some(parts.clone()),
                _ => None,
            }),
            _ => None,
        })
        .expect("a mixed class attribute exists");

    // The literal `a ` is preserved.
    assert!(
        parts
            .iter()
            .any(|p| matches!(p, MixedAttrPart::Literal(t) if t == "a ")),
        "the literal `a ` run is preserved (got {parts:?})"
    );
    // The `{b}` expression run captures the `b` reference.
    let expr_part = parts
        .iter()
        .find_map(|p| match p {
            MixedAttrPart::Expr(e) => Some(*e),
            _ => None,
        })
        .expect("the mixed value has an expression run");
    let refs: Vec<&str> = ir
        .analysis
        .expressions
        .get(expr_part)
        .references
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        refs.contains(&"b"),
        "the `{{b}}` run captures the `b` reference (got {refs:?})"
    );
}

// ---------------------------------------------------------------------------
// H6 — the mixed-attribute brace scan is JS-string / regex / comment aware.
//
// A byte-level `{`/`}` counter closes the interpolation at a `}` INSIDE a string
// literal (`class="x {format('}')} y"`), feeding broken text to OXC. The shared
// JS-aware brace scanner (the parser's `find_matching_brace_in`) keeps the
// interpolation open across braces inside strings / template literals / regexes /
// comments, so the whole expression is captured and the trailing literal survives.
// ---------------------------------------------------------------------------

mod mixed_attr_brace_awareness {
    use super::*;
    use crate::svelte::runtime::ir::{AttrIr, MixedAttrPart};

    /// The ordered parts of the first mixed attribute named `attr`.
    fn mixed_parts(ir: &super::super::ir::SvelteRuntimeIr, attr: &str) -> Vec<MixedAttrPart> {
        ir.nodes
            .iter()
            .find_map(|n| match n {
                IrNode::Element(el) => el.attrs.iter().find_map(|a| match a {
                    AttrIr::Mixed { name, parts } if name == attr => Some(parts.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .expect("a mixed attribute exists")
    }

    /// The source text of the single `{expr}` run of a mixed attribute.
    fn single_expr_source<'b>(
        ir: &'b super::super::ir::SvelteRuntimeIr,
        parts: &[MixedAttrPart],
    ) -> &'b str {
        let mut exprs = parts.iter().filter_map(|p| match p {
            MixedAttrPart::Expr(e) => Some(*e),
            _ => None,
        });
        let e = exprs.next().expect("one expression run");
        assert!(exprs.next().is_none(), "exactly one expression run");
        ir.analysis.expressions.get(e).source
    }

    #[test]
    fn brace_inside_single_quoted_string_does_not_close_interp() {
        let alloc = Allocator::default();
        // The `}` inside `'}'` must NOT close the interpolation. A byte counter
        // would split at the inner `}`, leaving a broken `format('` expression and
        // a stray `')} y` literal.
        let src =
            "<script>function format(s){return s}</script><div class=\"x {format('}')} y\">z</div>";
        let ir = lower(src, &alloc);
        let parts = mixed_parts(&ir, "class");
        assert_eq!(single_expr_source(&ir, &parts), "format('}')");
        // The trailing literal ` y` survives intact.
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, MixedAttrPart::Literal(t) if t == " y")),
            "the trailing ` y` literal survives (got {parts:?})"
        );
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, MixedAttrPart::Literal(t) if t == "x ")),
            "the leading `x ` literal survives (got {parts:?})"
        );
    }

    #[test]
    fn brace_inside_double_quoted_string_does_not_close_interp() {
        let alloc = Allocator::default();
        // A single-quoted attribute so a double-quoted JS string can hold `}`.
        let src = "<script>function f(s){return s}</script><div class='a {f(\"}\")} b'>z</div>";
        let ir = lower(src, &alloc);
        let parts = mixed_parts(&ir, "class");
        assert_eq!(single_expr_source(&ir, &parts), "f(\"}\")");
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, MixedAttrPart::Literal(t) if t == " b")),
            "the trailing ` b` literal survives (got {parts:?})"
        );
    }

    #[test]
    fn brace_inside_template_literal_does_not_close_interp() {
        let alloc = Allocator::default();
        // A backtick template literal holding a `}` must not close the interp.
        let src = "<script>let v = $state(1);</script><div class=\"p {`q}`+v} r\">z</div>";
        let ir = lower(src, &alloc);
        let parts = mixed_parts(&ir, "class");
        assert_eq!(single_expr_source(&ir, &parts), "`q}`+v");
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, MixedAttrPart::Literal(t) if t == " r")),
            "the trailing ` r` literal survives (got {parts:?})"
        );
    }

    /// The ordered LITERAL chunk strings of the first mixed attribute named `attr`.
    fn literal_chunks(ir: &super::super::ir::SvelteRuntimeIr, attr: &str) -> Vec<String> {
        mixed_parts(ir, attr)
            .into_iter()
            .filter_map(|p| match p {
                MixedAttrPart::Literal(s) => Some(s),
                MixedAttrPart::Expr(_) => None,
            })
            .collect()
    }

    #[test]
    fn mixed_attribute_literal_chunks_are_entity_decoded() {
        // X4 — a mixed-attribute LITERAL chunk is ENTITY-DECODED (the official
        // `decode_character_references`), NOT stored raw. svelte@5.56.3:
        // `title="&copy; {x} &bogus;"` → the runtime value `'© ' + x + ' &bogus;'`,
        // so the leading literal is `© ` (decoded) and the trailing is ` &bogus;`
        // (the unknown entity stays literal). FAILS against the prior raw-chunk
        // storage (`&copy; `).
        let alloc = Allocator::default();
        let src = "<script>let x=1;</script><div title=\"&copy; {x} &bogus;\">a</div>";
        let ir = lower(src, &alloc);
        let chunks = literal_chunks(&ir, "title");
        assert!(
            chunks.iter().any(|c| c == "\u{00a9} "),
            "the leading `&copy; ` literal must decode to `© ` (got {chunks:?})"
        );
        assert!(
            chunks.iter().any(|c| c == " &bogus;"),
            "the unknown `&bogus;` stays literal (got {chunks:?})"
        );
        // Negative: the raw `&copy;` must NOT survive as a literal chunk.
        assert!(
            !chunks.iter().any(|c| c.contains("&copy;")),
            "the raw `&copy;` entity must not survive undecoded (got {chunks:?})"
        );
    }

    #[test]
    fn mixed_attribute_literal_decode_is_not_reescaped() {
        // X4 — the decode is DECODE-ONLY (no re-escape): a mixed value is a runtime
        // STRING, never re-serialized HTML. svelte@5.56.3:
        // `title="&lt;a&gt; {x}"` → `'<a> ' + x`, so the literal decodes to `<a> `
        // and stays `<a> ` (NOT re-escaped back to `&lt;a&gt; `).
        let alloc = Allocator::default();
        let src = "<script>let x=1;</script><div title=\"&lt;a&gt; {x}\">a</div>";
        let ir = lower(src, &alloc);
        let chunks = literal_chunks(&ir, "title");
        assert!(
            chunks.iter().any(|c| c == "<a> "),
            "the literal `&lt;a&gt; ` must decode to `<a> ` and NOT be re-escaped (got {chunks:?})"
        );
    }

    #[test]
    fn mixed_attribute_numeric_entity_literal_is_decoded() {
        // X4 — a numeric entity in a mixed literal decodes too. svelte@5.56.3:
        // `title="&#65;{x}"` → `'A' + x`, so the leading literal is `A`.
        let alloc = Allocator::default();
        let src = "<script>let x=1;</script><div title=\"&#65;{x}\">a</div>";
        let ir = lower(src, &alloc);
        let chunks = literal_chunks(&ir, "title");
        assert!(
            chunks.iter().any(|c| c == "A"),
            "the numeric `&#65;` literal must decode to `A` (got {chunks:?})"
        );
    }

    #[test]
    fn mixed_attribute_amp_entity_literal_is_decoded() {
        // X4 — `&amp;` in a mixed literal decodes to a bare `&` (and stays `&`, not
        // re-escaped). svelte@5.56.3: `title="a &amp; b {x}"` → `'a & b ' + x`.
        let alloc = Allocator::default();
        let src = "<script>let x=1;</script><div title=\"a &amp; b {x}\">a</div>";
        let ir = lower(src, &alloc);
        let chunks = literal_chunks(&ir, "title");
        assert!(
            chunks.iter().any(|c| c == "a & b "),
            "the literal `a &amp; b ` must decode to `a & b ` (got {chunks:?})"
        );
    }
}

// ---------------------------------------------------------------------------
// F9 — an unknown `<svelte:bogus>` records a diagnostic; it is NOT coerced to a
// transparent Fragment. FAILS against the silent coercion.
// ---------------------------------------------------------------------------

#[test]
fn unknown_special_element_records_diagnostic() {
    let alloc = Allocator::default();
    let src = "<svelte:bogus>x</svelte:bogus>";
    let parsed = parse_svelte(src);
    let result = lower_parsed_svelte_to_ir(src, &parsed, &SvelteRuntimeOptions::default(), &alloc);
    let errors = result.expect_err("an unknown special element must fail lowering");
    assert!(
        errors
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-runtime-unknown-special-element"),
        "an unknown `<svelte:*>` surfaces a diagnostic (got {:?})",
        errors.diagnostics
    );
}

// ---------------------------------------------------------------------------
// F10 — the runtime ops arena is POPULATED for every reactive surface, attached
// to the owning scope's local_ops. FAILS against the empty arena / unpopulated
// local_ops.
// ---------------------------------------------------------------------------

#[test]
fn reactive_surfaces_emit_runtime_ops() {
    use super::ir::RuntimeOp;
    let alloc = Allocator::default();

    // `{count}` → ReactiveText.
    let src =
        "<script>let count = $state(0);</script><button onclick={() => count++}>{count}</button>";
    let ir = lower(src, &alloc);
    assert!(
        ir.ops
            .iter()
            .any(|o| matches!(o, RuntimeOp::ReactiveText { .. })),
        "a `{{count}}` interpolation emits a ReactiveText op"
    );
    // `onclick` → Event.
    assert!(
        ir.ops.iter().any(|o| matches!(o, RuntimeOp::Event { .. })),
        "an `onclick` emits an Event op"
    );
    // The ops are attached to a scope's local_ops (NOT an orphaned arena).
    assert!(
        ir.template_scopes.iter().any(|s| !s.local_ops.is_empty()),
        "emitted ops are attached to a scope's local_ops"
    );
    // Negative: a fully-static template emits NO ops.
    let static_ir = lower("<p>static</p>", &alloc);
    assert!(
        static_ir.ops.is_empty(),
        "a fully-static template emits no reactive ops"
    );

    // `bind:value` → Binding.
    let bind_ir = lower(
        "<script>let v = $state('');</script><input bind:value={v} />",
        &alloc,
    );
    assert!(
        bind_ir
            .ops
            .iter()
            .any(|o| matches!(o, RuntimeOp::Binding { .. })),
        "a `bind:value` emits a Binding op"
    );

    // `{...rest}` → SpreadAttrs.
    let spread_ir = lower(
        "<script>let rest = $state({});</script><div {...rest}>x</div>",
        &alloc,
    );
    assert!(
        spread_ir
            .ops
            .iter()
            .any(|o| matches!(o, RuntimeOp::SpreadAttrs { .. })),
        "a `{{...rest}}` emits a SpreadAttrs op"
    );

    // `{@attach fn}` → Attachment.
    let attach_ir = lower(
        "<script>let fn = () => {};</script><div>{@attach fn}</div>",
        &alloc,
    );
    assert!(
        attach_ir
            .ops
            .iter()
            .any(|o| matches!(o, RuntimeOp::Attachment { .. })),
        "an `{{@attach}}` emits an Attachment op"
    );
}

// ---------------------------------------------------------------------------
// G1 — NO reactive surface representable in the IR silently produces zero ops.
// Shorthand `class:`/`style:`/`bind:` directives synthesize their implied
// same-named expression; `Mixed` (`class="a {b}"`) emits a ReactiveAttr per
// expression run; `use:`/transition emit Action/Transition ops; attribute SOURCE
// ORDER is preserved (spreads are NOT hoisted). FAILS against the `_ => None`
// silent drop + the end-hoisted spread.
// ---------------------------------------------------------------------------

#[test]
fn shorthand_class_directive_emits_reactive_attr_op() {
    use super::ir::{AttrOpKind, RuntimeOp};
    let alloc = Allocator::default();
    // `class:active` (shorthand) — the condition is the implied `active` ref.
    let src = "<script>let active = $state(true);</script><div class:active>x</div>";
    let ir = lower(src, &alloc);
    let class_op = ir.ops.iter().find_map(|o| match o {
        RuntimeOp::ReactiveAttr { attr, .. } if attr.kind == AttrOpKind::Class => {
            Some(attr.clone())
        }
        _ => None,
    });
    let attr = class_op.expect("a shorthand `class:active` emits a Class ReactiveAttr op");
    assert_eq!(attr.name, "active", "the class name is `active`");
    // The synthesized expression references `active` (a real identifier).
    let refs: Vec<&str> = ir
        .analysis
        .expressions
        .get(attr.expr)
        .references
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        refs.contains(&"active"),
        "the synthesized shorthand expr references `active` (got {refs:?})"
    );
}

#[test]
fn shorthand_bind_directive_emits_binding_op() {
    use super::ir::RuntimeOp;
    let alloc = Allocator::default();
    // `bind:value` (shorthand) — the bound expression is the implied `value`.
    let src = "<script>let value = $state('');</script><input bind:value />";
    let ir = lower(src, &alloc);
    let bind = ir.ops.iter().find_map(|o| match o {
        RuntimeOp::Binding { bind, .. } => Some(bind.clone()),
        _ => None,
    });
    let bind = bind.expect("a shorthand `bind:value` emits a Binding op (not a dropped None)");
    assert_eq!(bind.target, "value", "the bind target is `value`");
    let refs: Vec<&str> = ir
        .analysis
        .expressions
        .get(bind.expr)
        .references
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        refs.contains(&"value"),
        "the synthesized bind expr references `value` (got {refs:?})"
    );
}

#[test]
fn shorthand_style_directive_emits_reactive_attr_op() {
    use super::ir::{AttrOpKind, RuntimeOp};
    let alloc = Allocator::default();
    let src = "<script>let color = $state('red');</script><div style:color>x</div>";
    let ir = lower(src, &alloc);
    assert!(
        ir.ops.iter().any(|o| matches!(
            o,
            RuntimeOp::ReactiveAttr { attr, .. } if attr.kind == AttrOpKind::Style && attr.name == "color"
        )),
        "a shorthand `style:color` emits a Style ReactiveAttr op"
    );
}

#[test]
fn mixed_attribute_value_emits_reactive_attr_op() {
    use super::ir::{AttrOpKind, RuntimeOp};
    let alloc = Allocator::default();
    // `class="a {b}"` — a Mixed value with an expression run emits a ReactiveAttr.
    let src = "<script>let b = $state('y');</script><div class=\"a {b}\">x</div>";
    let ir = lower(src, &alloc);
    assert!(
        ir.ops.iter().any(|o| matches!(
            o,
            RuntimeOp::ReactiveAttr { attr, .. } if attr.kind == AttrOpKind::Class
        )),
        "a `class=\"a {{b}}\"` Mixed value emits a Class ReactiveAttr op (NOT a silent drop)"
    );
}

#[test]
fn use_action_emits_action_op() {
    use super::ir::RuntimeOp;
    let alloc = Allocator::default();
    // `use:fn` (no arg) — must emit an Action op (the action ref is `fn`).
    let src = "<script>function fn(){}</script><div use:fn>x</div>";
    let ir = lower(src, &alloc);
    let action = ir.ops.iter().find_map(|o| match o {
        RuntimeOp::Action { action, .. } => Some(action.clone()),
        _ => None,
    });
    let action = action.expect("a `use:fn` emits an Action op (not a dropped None)");
    assert!(action.arg.is_none(), "a bare `use:fn` has no argument");
    let refs: Vec<&str> = ir
        .analysis
        .expressions
        .get(action.expr)
        .references
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        refs.contains(&"fn"),
        "the action expr references `fn` (got {refs:?})"
    );

    // `use:fn={arg}` — carries the argument.
    let src2 = "<script>function fn(){} let arg = $state(1);</script><div use:fn={arg}>x</div>";
    let ir2 = lower(src2, &alloc);
    let action2 = ir2
        .ops
        .iter()
        .find_map(|o| match o {
            RuntimeOp::Action { action, .. } => Some(action.clone()),
            _ => None,
        })
        .expect("a `use:fn={arg}` emits an Action op");
    assert!(
        action2.arg.is_some(),
        "a `use:fn={{arg}}` carries the argument"
    );
}

#[test]
fn transition_directive_emits_transition_op() {
    use super::ir::{RuntimeOp, TransitionKind};
    let alloc = Allocator::default();
    let src = "<script>import { fade } from 'svelte/transition'; let show = $state(true);</script>{#if show}<div transition:fade>x</div>{/if}";
    let ir = lower(src, &alloc);
    assert!(
        ir.ops.iter().any(|o| matches!(
            o,
            RuntimeOp::Transition { transition, .. }
                if transition.kind == TransitionKind::Transition
                    && transition.name == "fade"
                    && !transition.global
        )),
        "a `transition:fade` emits a Transition op (no `|global` modifier ⇒ global false)"
    );
}

#[test]
fn transition_global_modifier_is_typed_local_is_default() {
    // The `|global` modifier is a TYPED lowering fact (the official
    // `TRANSITION_GLOBAL` bit source); `|local` (and no modifier) is the default
    // `false`. One element per kind so the three kinds' flags stay distinct.
    use super::ir::{RuntimeOp, TransitionKind};
    let alloc = Allocator::default();
    let src = "<script>let c = $state(0);</script><div in:fade|global onclick={() => c++}>x</div><p out:fly|local>y</p><a transition:blur>z</a>";
    let ir = lower(src, &alloc);
    let transitions: Vec<_> = ir
        .ops
        .iter()
        .filter_map(|o| match o {
            RuntimeOp::Transition { transition, .. } => Some(transition.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(transitions.len(), 3, "three transition ops lower");
    assert!(
        transitions
            .iter()
            .any(|t| t.kind == TransitionKind::In && t.name == "fade" && t.global),
        "`in:fade|global` carries global: true"
    );
    assert!(
        transitions
            .iter()
            .any(|t| t.kind == TransitionKind::Out && t.name == "fly" && !t.global),
        "`out:fly|local` is the DEFAULT (global: false — `|local` adds no bit)"
    );
    assert!(
        transitions
            .iter()
            .any(|t| t.kind == TransitionKind::Transition && t.name == "blur" && !t.global),
        "a bare `transition:blur` carries global: false"
    );
}

#[test]
fn animate_directive_emits_animation_op_not_transition() {
    // `animate:` is its OWN op family (`RuntimeOp::Animation` → `$.animation`) —
    // NEVER a Transition op masquerade. Both the no-params and params forms lower.
    use super::ir::RuntimeOp;
    let alloc = Allocator::default();
    let src = "<script>let { items } = $props();</script>{#each items as item (item.id)}<div animate:flip>{item}</div>{/each}";
    let ir = lower(src, &alloc);
    let animation = ir
        .ops
        .iter()
        .find_map(|o| match o {
            RuntimeOp::Animation { animation, .. } => Some(animation.clone()),
            _ => None,
        })
        .expect("an `animate:flip` emits an Animation op");
    assert_eq!(animation.name, "flip");
    assert!(animation.expr.is_none(), "no params ⇒ no expr");
    assert!(
        !ir.ops
            .iter()
            .any(|o| matches!(o, RuntimeOp::Transition { .. })),
        "an `animate:` directive must NOT lower to a Transition op"
    );

    let src2 = "<script>let { items } = $props();</script>{#each items as item (item.id)}<div animate:flip={{ duration: 200 }}>{item}</div>{/each}";
    let ir2 = lower(src2, &alloc);
    let animation2 = ir2
        .ops
        .iter()
        .find_map(|o| match o {
            RuntimeOp::Animation { animation, .. } => Some(animation.clone()),
            _ => None,
        })
        .expect("an `animate:flip={{…}}` emits an Animation op");
    assert!(animation2.expr.is_some(), "params ⇒ the expr is carried");
}

#[test]
fn attach_attribute_position_lowers_to_attach_attr_and_attachment_op() {
    // ELEMENT-position `{@attach expr}` is a dedicated ATTRIBUTE kind
    // (`AttrIr::Attach` → `RuntimeOp::Attachment`) with the expression span captured
    // by the tokenizer — NOT the empty-name-Plain fallthrough (whose `@attach fn`
    // body would fail the expression parse) and NOT the child-form `TagIr::Attach`.
    use super::ir::{AttrIr, IrNode, RuntimeOp};
    let alloc = Allocator::default();
    let src = "<script>let c = $state(0);</script><div {@attach fn} onclick={() => c++}>x</div>";
    let ir = lower(src, &alloc);
    let attach_expr = ir
        .nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Element(el) => el.attrs.iter().find_map(|a| match a {
                AttrIr::Attach { expr } => Some(*expr),
                _ => None,
            }),
            _ => None,
        })
        .expect("an element `{@attach fn}` lowers to AttrIr::Attach");
    // The captured expression is exactly `fn` (the span excludes the keyword).
    assert_eq!(ir.analysis.expressions.get(attach_expr).source.trim(), "fn");
    assert!(
        ir.ops
            .iter()
            .any(|o| matches!(o, RuntimeOp::Attachment { expr, .. } if *expr == attach_expr)),
        "the element `{{@attach}}` emits an Attachment op carrying the captured expr"
    );
    // NEGATIVE: no empty-name Dynamic attr leaked from the old Plain fallthrough.
    assert!(
        !ir.nodes.iter().any(|n| matches!(
            n,
            IrNode::Element(el) if el.attrs.iter().any(
                |a| matches!(a, AttrIr::Dynamic { name, .. } if name.is_empty())
            )
        )),
        "no empty-name Dynamic attr may leak from the retired Plain fallthrough"
    );
}

#[test]
fn attribute_ops_preserve_source_order_spread_not_hoisted() {
    use super::ir::RuntimeOp;
    let alloc = Allocator::default();
    // `<div {...rest} id={x}>` — the spread precedes the dynamic attribute in
    // source, so the SpreadAttrs op must be emitted BEFORE the ReactiveAttr op.
    // FAILS against the pre-fix code that hoisted all spreads to the end.
    let src =
        "<script>let rest = $state({}); let x = $state(1);</script><div {...rest} id={x}>y</div>";
    let ir = lower(src, &alloc);
    let positions: Vec<&'static str> = ir
        .ops
        .iter()
        .filter_map(|o| match o {
            RuntimeOp::SpreadAttrs { .. } => Some("spread"),
            RuntimeOp::ReactiveAttr { .. } => Some("attr"),
            _ => None,
        })
        .collect();
    assert_eq!(
        positions,
        vec!["spread", "attr"],
        "the spread op precedes the dynamic-attr op in source order (got {positions:?})"
    );

    // Control: reversed source order → reversed op order.
    let src2 =
        "<script>let rest = $state({}); let x = $state(1);</script><div id={x} {...rest}>y</div>";
    let ir2 = lower(src2, &alloc);
    let positions2: Vec<&'static str> = ir2
        .ops
        .iter()
        .filter_map(|o| match o {
            RuntimeOp::SpreadAttrs { .. } => Some("spread"),
            RuntimeOp::ReactiveAttr { .. } => Some("attr"),
            _ => None,
        })
        .collect();
    assert_eq!(
        positions2,
        vec!["attr", "spread"],
        "attr-before-spread source order is preserved (got {positions2:?})"
    );
}

// ---------------------------------------------------------------------------
// G3 — a special-element event listener targets the global the element
// represents (`<svelte:window>` ⇒ Window, etc.), NOT the node. FAILS against the
// always-`EventTarget::Node(target)` emission.
// ---------------------------------------------------------------------------

#[test]
fn special_element_events_target_the_global() {
    use super::ir::{EventTarget, RuntimeOp};
    let alloc = Allocator::default();

    let window_target = |src: &str, name: &str| -> EventTarget {
        let parsed = parse_svelte(src);
        let ir = lower_parsed_svelte_to_ir(src, &parsed, &SvelteRuntimeOptions::default(), &alloc)
            .expect("lowering succeeds");
        ir.ops
            .iter()
            .find_map(|o| match o {
                RuntimeOp::Event { target, .. } => Some(*target),
                _ => None,
            })
            .unwrap_or_else(|| panic!("an Event op exists for {name}"))
    };

    assert_eq!(
        window_target(
            "<script>function h(){}</script><svelte:window onresize={h}/>",
            "window"
        ),
        EventTarget::Window,
        "a <svelte:window> event targets Window"
    );
    assert_eq!(
        window_target(
            "<script>function h(){}</script><svelte:document onkeydown={h}/>",
            "document"
        ),
        EventTarget::Document,
        "a <svelte:document> event targets Document"
    );
    assert_eq!(
        window_target(
            "<script>function h(){}</script><svelte:body onclick={h}/>",
            "body"
        ),
        EventTarget::Body,
        "a <svelte:body> event targets Body"
    );

    // Negative: an intrinsic element's event still targets the Node, not a global.
    let src = "<script>function h(){}</script><button onclick={h}>x</button>";
    let parsed = parse_svelte(src);
    let ir = lower_parsed_svelte_to_ir(src, &parsed, &SvelteRuntimeOptions::default(), &alloc)
        .expect("lowering succeeds");
    let target = ir
        .ops
        .iter()
        .find_map(|o| match o {
            RuntimeOp::Event { target, .. } => Some(*target),
            _ => None,
        })
        .expect("an Event op exists");
    assert!(
        matches!(target, EventTarget::Node(_)),
        "an intrinsic element event targets the Node (got {target:?})"
    );
}

// ---------------------------------------------------------------------------
// F11 — a `{@render row(1)}` static snippet callee resolves to RenderCallee::
// Snippet + the parsed arg expr; `{@render getSnippet()?.()}` stays Dynamic.
// FAILS against the always-Dynamic-empty-args lowering.
// ---------------------------------------------------------------------------

#[test]
fn render_static_snippet_callee_resolves_to_snippet_with_args() {
    use super::ir::RenderCallee;
    let alloc = Allocator::default();
    // `row` is a local `{#snippet}` — `{@render row(1)}` is a static snippet call.
    let src = "{#snippet row(n)}<li>{n}</li>{/snippet}{@render row(1)}";
    let ir = lower(src, &alloc);
    let (callee, args) = ir
        .nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Tag(TagIr::Render { callee, args, .. }) => Some((callee.clone(), args.clone())),
            _ => None,
        })
        .expect("a render tag exists");
    assert!(
        matches!(callee, RenderCallee::Snippet { .. }),
        "a static snippet callee resolves to RenderCallee::Snippet (got {callee:?})"
    );
    assert_eq!(args.len(), 1, "`{{@render row(1)}}` carries one argument");
}

#[test]
fn render_optional_call_stays_dynamic() {
    use super::ir::RenderCallee;
    let alloc = Allocator::default();
    // An optional call `getSnippet()?.()` stays Dynamic.
    let src = "<script>let getSnippet = () => {};</script>{@render getSnippet()?.()}";
    let ir = lower(src, &alloc);
    let callee = ir
        .nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Tag(TagIr::Render { callee, .. }) => Some(callee.clone()),
            _ => None,
        })
        .expect("a render tag exists");
    assert!(
        matches!(callee, RenderCallee::Dynamic(_)),
        "an optional-call render callee stays Dynamic (got {callee:?})"
    );
    assert!(
        !matches!(callee, RenderCallee::Snippet { .. }),
        "an optional-call render callee must NOT resolve to a static Snippet"
    );
}

#[test]
fn render_member_callee_carries_args() {
    use super::ir::RenderCallee;
    let alloc = Allocator::default();
    // A MEMBER-callee render `{@render obj.snip(item)}` stays Dynamic (`obj.snip` is not a
    // `{#snippet}` name) but MUST keep its argument expression — the official emits
    // `$.snippet(node, () => $$props.obj.snip, () => $$props.item)`. This is the member-callee
    // half of the dynamic-render-arg class (covered by IR here rather than a golden fixture
    // because the official wraps the member-callee component in a `$.push`/`$.pop` context, an
    // orthogonal `needs_context` behaviour). FAILS against the always-empty-args lowering.
    let src = "<script>let { obj, item } = $props();</script>{@render obj.snip(item)}";
    let ir = lower(src, &alloc);
    let (callee, args) = ir
        .nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Tag(TagIr::Render { callee, args, .. }) => Some((callee.clone(), args.clone())),
            _ => None,
        })
        .expect("a render tag exists");
    assert!(
        matches!(callee, RenderCallee::Dynamic(_)),
        "a member-callee render stays Dynamic (got {callee:?})"
    );
    assert_eq!(
        args.len(),
        1,
        "`{{@render obj.snip(item)}}` carries its one argument thunk"
    );
}

// ---------------------------------------------------------------------------
// The stored `render_callee` fact — classified ONCE by the SAME parse that
// analyzes the expression (`collect_expr_references`), the single authority
// the render-callee resolution pass AND the CSS matcher read (no consumer
// re-parses the inner text). FAILS against an always-Dynamic-empty stub.
// ---------------------------------------------------------------------------

/// The stored `render_callee` fact of one expression source (the same
/// single-parse analysis `push_expr` runs).
fn render_callee_fact_of(src: &str) -> super::expr::RenderCalleeShape {
    super::expr::collect_expr_references(src)
        .expect("the render expression parses")
        .render_callee
}

#[test]
fn render_callee_fact_static_name_carries_optional_flag_and_arg_spans() {
    use super::expr::RenderCalleeShape;
    // `row(1)` — the arg span is INNER-TEXT-relative (the `1` at 4..5).
    assert_eq!(
        render_callee_fact_of("row(1)"),
        RenderCalleeShape::StaticName {
            name: "row".to_string(),
            optional: false,
            args: vec![(4, 5)],
        }
    );
    // The optional call keeps the flag; author parens around the callee peel.
    assert_eq!(
        render_callee_fact_of("row?.(1)"),
        RenderCalleeShape::StaticName {
            name: "row".to_string(),
            optional: true,
            args: vec![(6, 7)],
        }
    );
    assert_eq!(
        render_callee_fact_of("(row)(1)"),
        RenderCalleeShape::StaticName {
            name: "row".to_string(),
            optional: false,
            args: vec![(6, 7)],
        }
    );
}

#[test]
fn render_callee_fact_member_and_non_call_shapes_stay_dynamic() {
    use super::expr::RenderCalleeShape;
    // A member callee stays Dynamic but keeps its argument span (`x` at 9..10).
    assert_eq!(
        render_callee_fact_of("obj.snip(x)"),
        RenderCalleeShape::Dynamic {
            args: vec![(9, 10)],
        }
    );
    // A non-call expression is the whole dynamic callee, with no args.
    assert_eq!(
        render_callee_fact_of("row"),
        RenderCalleeShape::Dynamic { args: Vec::new() }
    );
}

#[test]
fn render_callee_fact_spread_argument_flags_fail_closed() {
    use super::expr::RenderCalleeShape;
    // A spread argument is the official `render_tag_invalid_spread_argument`
    // hard error — the fact carries the fail-closed marker, even through
    // author parens around the whole call.
    assert_eq!(
        render_callee_fact_of("row(...xs)"),
        RenderCalleeShape::SpreadArguments
    );
    assert_eq!(
        render_callee_fact_of("(row(...xs))"),
        RenderCalleeShape::SpreadArguments
    );
}

// ---------------------------------------------------------------------------
// The stored wrap-trigger + dynamic-callee lowering facts — populated by the
// SAME canonical parse (`collect_expr_references`), the single authority the
// legacy wrap trigger and the dynamic `{@render}` lowering read (no consumer
// re-parses / re-slices / re-collects). Torn expressions carry FAIL-CLOSED
// facts, never silent defaults.
// ---------------------------------------------------------------------------

/// The per-parse analysis facts of one expression source.
fn analysis_facts_of(src: &str) -> super::expr::ExprAnalysisFacts {
    super::expr::collect_expr_references(src).expect("the expression parses")
}

#[test]
fn sync_member_or_assignment_fact_covers_member_assignment_update() {
    // Members (incl. global-rooted), assignments, and updates trigger.
    assert!(analysis_facts_of("obj.x").has_sync_member_or_assignment);
    assert!(analysis_facts_of("Math.PI").has_sync_member_or_assignment);
    assert!(analysis_facts_of("x = 1").has_sync_member_or_assignment);
    assert!(analysis_facts_of("x++").has_sync_member_or_assignment);
    assert!(analysis_facts_of("(a.x, () => b.y)").has_sync_member_or_assignment);
    // A plain read / call / binary carries no member/assignment trigger.
    assert!(!analysis_facts_of("x").has_sync_member_or_assignment);
    assert!(!analysis_facts_of("f(x)").has_sync_member_or_assignment);
    assert!(!analysis_facts_of("a + b").has_sync_member_or_assignment);
    // A nested fn/arrow body is DEFERRED (official nulls `state.expression`).
    assert!(!analysis_facts_of("() => obj.x").has_sync_member_or_assignment);
    assert!(!analysis_facts_of("function f() { return obj.x; }").has_sync_member_or_assignment);
}

#[test]
fn render_dynamic_callee_fact_carries_span_chain_and_subtree_facts() {
    // `children?.()` — the callee span slices to `children`, the chain flag is
    // set, and the callee subtree's facts are its own (a bare identifier: one
    // read reference, no sync trigger, no zero-arg-call fact, a root ident).
    let src = "children?.()";
    let facts = analysis_facts_of(src)
        .render_dynamic_callee
        .expect("a trailing call populates the fact");
    assert_eq!(
        &src[facts.span.0 as usize..facts.span.1 as usize],
        "children",
        "the populated span slices the callee text"
    );
    assert!(facts.is_chain, "`?.()` is the chain form");
    assert_eq!(facts.references.len(), 1);
    assert_eq!(facts.references[0].name, "children");
    assert!(!facts.has_sync_member_or_assignment);
    assert_eq!(facts.direct_zero_arg_call_callee, None);
    assert_eq!(facts.root_ident.as_deref(), Some("children"));
    // `obj.snip(x)` — a member callee: plain call, member trigger inside the
    // callee subtree, the ARGUMENT's reference is NOT a callee reference.
    let src = "obj.snip(x)";
    let facts = analysis_facts_of(src)
        .render_dynamic_callee
        .expect("a trailing call populates the fact");
    assert_eq!(
        &src[facts.span.0 as usize..facts.span.1 as usize],
        "obj.snip"
    );
    assert!(!facts.is_chain);
    assert!(facts.has_sync_member_or_assignment);
    assert_eq!(facts.references.len(), 1);
    assert_eq!(facts.references[0].name, "obj");
    assert_eq!(facts.root_ident, None);
    // `f()()` — a call-of-call: the callee slice is itself a zero-arg
    // identifier call (the unthunk fact of the SLICE).
    let src = "f()()";
    let facts = analysis_facts_of(src)
        .render_dynamic_callee
        .expect("a trailing call populates the fact");
    assert_eq!(&src[facts.span.0 as usize..facts.span.1 as usize], "f()");
    assert_eq!(facts.direct_zero_arg_call_callee.as_deref(), Some("f"));
    // A NON-call expression populates no fact — the render lowering fails
    // closed on it.
    assert_eq!(
        analysis_facts_of("cond ? a : b").render_dynamic_callee,
        None
    );
}

#[test]
fn torn_expression_facts_are_fail_closed_not_defaults() {
    // A torn expression's wrap-trigger fact is `Err(())` — the preparation
    // entry maps it to the precise
    // `svelte-runtime-unsupported-expression-fact-recovery` diagnostic instead
    // of silently treating the surface as raw (`false` would).
    let torn = super::expr::AnalyzedExpr::torn("a ..b", super::expr::ScopeId(0));
    assert_eq!(torn.has_sync_member_or_assignment, Err(()));
    assert_eq!(torn.render_dynamic_callee, None);
    assert!(torn.render_callee.is_err());
    // An interned expression carries the populated fact.
    let facts = analysis_facts_of("obj.x");
    let interned = super::expr::AnalyzedExpr::interned("obj.x", super::expr::ScopeId(0), facts);
    assert_eq!(interned.has_sync_member_or_assignment, Ok(true));
}

// ---------------------------------------------------------------------------
// F12 — a zero-root template (only a `<script>` + whitespace) still plans a
// CommentAnchor for the root region. FAILS against the skip-empty-region path
// that produced no factory.
// ---------------------------------------------------------------------------

#[test]
fn zero_root_template_plans_comment_anchor() {
    let alloc = Allocator::default();
    let src = "<script>let n = $state(0);</script>\n";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    assert!(
        !plan.templates.is_empty(),
        "a zero-root component still plans a root factory"
    );
    assert!(
        matches!(
            plan.templates[0],
            TemplateFactory::CommentAnchor {
                reason: AnchorReason::EmptyRoot
            }
        ),
        "a zero-root component plans an EmptyRoot comment anchor (got {:?})",
        plan.templates[0]
    );
}

// ---------------------------------------------------------------------------
// F12 — client_paths are planned from the EFFECTIVE DOM sequence, so a dynamic
// node behind leading whitespace gets the right path (not a desynced offset).
// ---------------------------------------------------------------------------

#[test]
fn client_paths_built_from_effective_dom_sequence() {
    use super::html::{NodePathStep, PathBase};
    let alloc = Allocator::default();
    // `<div>\n{x}</div>` — a SINGLE-root element template. The clone-root variable IS
    // the `<div>` (official `Fragment.js` `is_single_element`: a single-element
    // `$.from_html` returns the element, not a fragment), so the `<div>` carries NO
    // DOM-walk path of its own, and the interpolation descends DIRECTLY from the
    // clone-root via `$.child` — the path base is the clone-root `Fragment` with a
    // single `Child` step (the leading whitespace is normalized out of the effective
    // sequence, so there is NO sibling offset).
    let src = "<script>let x = $state(0);</script><div>\n{x}</div>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    // The interpolation node's path.
    let interp = ir
        .nodes
        .iter()
        .position(|n| matches!(n, IrNode::Interpolation { .. }))
        .map(|i| super::ir::NodeId(i as u32))
        .expect("an interpolation node exists");
    let path = plan
        .client_paths
        .iter()
        .find(|p| p.node == interp)
        .expect("the interpolation has a client path");
    // The interpolation descends from the clone-root element directly: base
    // `Fragment` (the clone-root), a single `Child` step, NO sibling offset.
    assert_eq!(
        path.base,
        PathBase::Fragment,
        "the interpolation descends from the clone-root (Fragment base) — got {:?}",
        path.base
    );
    assert_eq!(
        path.steps,
        vec![NodePathStep::Child { transparent: false }],
        "the single-root clone descendant is a single Child from the clone root (got {:?})",
        path.steps
    );
    // The single-root `<div>` is the clone variable itself — it gets NO path of its
    // own (the official zero-walk root). FAILS against the pre-fix code that emitted
    // a `[FirstChild]` path for the single-root div.
    let div = ir
        .nodes
        .iter()
        .position(|n| matches!(n, IrNode::Element(_)))
        .map(|i| super::ir::NodeId(i as u32))
        .expect("a div element node exists");
    assert!(
        plan.client_paths.iter().all(|p| p.node != div),
        "the single-root clone-root div carries NO DOM-walk path of its own"
    );
}

// ---------------------------------------------------------------------------
// G5 — every NodePathPlan is self-contained from its PathBase: a Fragment base
// reaches roots[i] via FirstChild then Sibling{i}; a Node base only names a node
// that has its OWN reachable plan. FAILS against the pre-fix bare-Sibling
// multi-root path and the unreachable Node base.
// ---------------------------------------------------------------------------

#[test]
fn multiroot_dynamic_after_static_root_descends_from_fragment_first() {
    use super::html::{NodePathStep, PathBase};
    let alloc = Allocator::default();
    // `<a/>{x}` — a static element root followed by a dynamic interpolation root.
    // svelte@5.56.3 reaches the interpolation via `$.sibling($.first_child(frag), 1)`
    // — FirstChild THEN Sibling{1}. FAILS against the bare `Sibling{1}` path.
    let src = "<script>let x = $state(0);</script><a href=\"/\">link</a>{x}";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let interp = ir
        .nodes
        .iter()
        .position(|n| matches!(n, IrNode::Interpolation { .. }))
        .map(|i| super::ir::NodeId(i as u32))
        .expect("an interpolation node exists");
    let path = plan
        .client_paths
        .iter()
        .find(|p| p.node == interp)
        .expect("the interpolation has a client path");
    assert_eq!(
        path.base,
        PathBase::Fragment,
        "a top-level dynamic root roots at the Fragment"
    );
    assert_eq!(
        path.steps,
        vec![
            NodePathStep::FirstChild,
            NodePathStep::Sibling { offset: 1 }
        ],
        "roots[1] is reached via FirstChild THEN Sibling{{1}} (got {:?})",
        path.steps
    );
}

#[test]
fn every_node_base_path_refers_to_a_node_with_its_own_plan() {
    use super::html::PathBase;
    let alloc = Allocator::default();
    // A nested structure: a dynamic interpolation deep inside static elements, plus
    // a multi-root layout — exercise both the Fragment-base and Node-base paths.
    let src = "<script>let x = $state(0); let y = $state(1);</script><section><div><span>{x}</span></div></section>{y}";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    // Every PathBase::Node(n) must refer to a node that has its OWN path plan.
    for path in &plan.client_paths {
        if let PathBase::Node(base) = path.base {
            assert!(
                plan.client_paths.iter().any(|p| p.node == base),
                "Node base {base:?} of path for {:?} has no own plan (unreachable)",
                path.node
            );
        }
    }
    // And every dynamic node (interpolation) requiring DOM reachability has a path.
    for (i, node) in ir.nodes.iter().enumerate() {
        if matches!(node, IrNode::Interpolation { .. }) {
            let id = super::ir::NodeId(i as u32);
            assert!(
                plan.client_paths.iter().any(|p| p.node == id),
                "the interpolation node {id:?} has a client path"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// F13 — the SSR-reusable dynamic-slot list represents every dynamic surface:
// {@html}, spreads, class, style, binds — not just text + plain attributes.
// ---------------------------------------------------------------------------

#[test]
fn dynamic_slot_list_covers_every_dynamic_surface() {
    use super::html::DynamicSlotKind;
    let alloc = Allocator::default();

    let html_ir = lower(
        "<script>let h = $state('<b>x</b>');</script><div>{@html h}</div>",
        &alloc,
    );
    let html_slots = plan_static_templates(&html_ir, None).slots;
    assert!(
        html_slots
            .iter()
            .any(|s| matches!(s.kind, DynamicSlotKind::Html { .. })),
        "an `{{@html}}` produces an Html slot"
    );

    let spread_ir = lower(
        "<script>let rest = $state({});</script><div {...rest}>x</div>",
        &alloc,
    );
    assert!(
        plan_static_templates(&spread_ir, None)
            .slots
            .iter()
            .any(|s| matches!(s.kind, DynamicSlotKind::Spread { .. })),
        "a `{{...rest}}` produces a Spread slot"
    );

    let class_ir = lower(
        "<script>let on = $state(true);</script><div class:active={on}>x</div>",
        &alloc,
    );
    assert!(
        plan_static_templates(&class_ir, None)
            .slots
            .iter()
            .any(|s| matches!(s.kind, DynamicSlotKind::Class { .. })),
        "a `class:` directive produces a Class slot"
    );

    let style_ir = lower(
        "<script>let c = $state('red');</script><div style:color={c}>x</div>",
        &alloc,
    );
    assert!(
        plan_static_templates(&style_ir, None)
            .slots
            .iter()
            .any(|s| matches!(s.kind, DynamicSlotKind::Style { .. })),
        "a `style:` directive produces a Style slot"
    );

    let bind_ir = lower(
        "<script>let v = $state('');</script><input bind:value={v} />",
        &alloc,
    );
    assert!(
        plan_static_templates(&bind_ir, None)
            .slots
            .iter()
            .any(|s| matches!(s.kind, DynamicSlotKind::Bind { .. })),
        "a `bind:value` produces a Bind slot"
    );
}

// ---------------------------------------------------------------------------
// G6 — the SSR dynamic-slot list includes COMPONENT props and SPECIAL-element
// dynamic attrs / binds, not just intrinsic-element attributes. FAILS against
// the pre-fix code that recursed component / special-element children only and
// dropped their dynamic attrs from the shared SSR surface.
// ---------------------------------------------------------------------------

#[test]
fn component_dynamic_prop_contributes_a_slot() {
    use super::html::DynamicSlotKind;
    let alloc = Allocator::default();
    // `<Foo value={v} />` — the dynamic prop `value` must appear in the dynamic
    // slot list (the server renders it).
    let src =
        "<script>import Foo from './Foo.svelte'; let v = $state(1);</script><Foo value={v} />";
    let ir = lower(src, &alloc);
    let slots = plan_static_templates(&ir, None).slots;
    assert!(
        slots.iter().any(|s| matches!(
            &s.kind,
            DynamicSlotKind::Attribute { name, .. } if name == "value"
        )),
        "a component's dynamic prop contributes an Attribute slot (got {slots:?})"
    );
}

#[test]
fn special_element_dynamic_bind_contributes_a_slot() {
    use super::html::DynamicSlotKind;
    let alloc = Allocator::default();
    // `<svelte:window bind:innerWidth={w} />` — the dynamic bind must contribute a
    // slot from the special element's attrs.
    let src = "<script>let w = $state(0);</script><svelte:window bind:innerWidth={w} />";
    let ir = lower(src, &alloc);
    let slots = plan_static_templates(&ir, None).slots;
    assert!(
        slots.iter().any(|s| matches!(
            &s.kind,
            DynamicSlotKind::Bind { target, .. } if target == "innerWidth"
        )),
        "a special element's dynamic bind contributes a Bind slot (got {slots:?})"
    );
}

// ---------------------------------------------------------------------------
// H4 — non-body special elements (`<svelte:head>` / `<svelte:options>` /
// window / document / body) are EXCLUDED from the body static-HTML skeleton: they
// are not roots, not `<!>` anchors, and never shift a sibling's body position or
// trip the multi-root fragment flag. (The full head/window/options LOWERING is a
// downstream layer — see the deferral ledger in the topology oracle.)
// ---------------------------------------------------------------------------

mod non_body_special_excluded_from_skeleton {
    use super::*;

    fn skeletons(src: &str) -> Vec<(String, Option<String>)> {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        plan_static_templates(&ir, None)
            .templates
            .iter()
            .filter_map(|t| match t {
                TemplateFactory::FromHtml {
                    html,
                    fragment_flag,
                    ..
                } => Some((html.clone(), fragment_flag.map(|f| f.literal()))),
                TemplateFactory::TextNode { .. }
                | TemplateFactory::CommentAnchor { .. }
                | TemplateFactory::Standalone { .. } => None,
            })
            .collect()
    }

    #[test]
    fn svelte_head_before_div_produces_div_only_body_no_anchor_no_flag() {
        // official svelte@5.56.3: `<svelte:head>…</svelte:head><div>{@html m}</div>`
        // → body skeleton "<div></div>" (single root, NO fragment flag, NO `<!>`
        // anchor for the head). The pre-fix code emitted "<!> <div></div>", 1.
        let src = "<script>let t=$state('x'); let m=$state('y');</script>\n<svelte:head><title>{t}</title></svelte:head>\n<div>{@html m}</div>";
        let skel = skeletons(src);
        assert!(
            skel.iter()
                .any(|(h, flag)| h == "<div></div>" && flag.is_none()),
            "the body skeleton is a single-root `<div></div>` with no fragment flag (got {skel:?})"
        );
        // Negative: NO body region carries a `<!>` head anchor.
        assert!(
            !skel.iter().any(|(h, _)| h.starts_with("<!>")),
            "the `<svelte:head>` must NOT produce a `<!>` body anchor (got {skel:?})"
        );
    }

    #[test]
    fn svelte_window_before_div_is_single_root_no_flag() {
        // A `<svelte:window>` before a `<div>` leaves a single body root.
        let src = "<script>let w=$state(0);</script>\n<svelte:window bind:innerWidth={w} />\n<div>x</div>";
        let skel = skeletons(src);
        assert_eq!(
            skel,
            vec![("<div>x</div>".to_string(), None)],
            "a `<svelte:window>` is excluded from the body skeleton (single root, no flag)"
        );
    }

    #[test]
    fn svelte_options_namespace_does_not_occupy_a_body_root() {
        // `<svelte:options namespace="svg" />` before a `<circle>` leaves a single
        // body root (the circle). (The `from_svg` root-helper selection itself is the
        // namespace-aware root-helper selection layer.)
        let src = "<svelte:options namespace=\"svg\" />\n<script>let r=$state(10);</script>\n<circle r={r} />";
        let skel = skeletons(src);
        assert_eq!(
            skel.len(),
            1,
            "only the `<circle>` is a body root, the options element is excluded (got {skel:?})"
        );
        assert_eq!(skel[0].1, None, "single body root carries no fragment flag");
    }

    #[test]
    fn renderable_special_element_still_keeps_its_anchor() {
        // Contrast: `<svelte:element this={tag}>` IS renderable body content — it
        // keeps its `<!>` anchor (NOT excluded). With a following `<div>`, the body
        // is a 2-root fragment.
        let src = "<script>let tag=$state('span');</script>\n<svelte:element this={tag}>hi</svelte:element>\n<div>x</div>";
        let skel = skeletons(src);
        assert!(
            skel.iter().any(|(h, flag)| h.starts_with("<!>") && flag.as_deref() == Some("1")),
            "a renderable `<svelte:element>` keeps its `<!>` anchor in a multi-root body (got {skel:?})"
        );
    }

    #[test]
    fn svelte_element_this_is_the_dynamic_tag_fact_not_an_attribute() {
        // `<svelte:element this={tag}>` carries `tag` as the distinct `this_expr`
        // dynamic-tag fact — NOT a `this` DOM attribute. The `this` attribute is
        // REMOVED from the generic attribute list (official reads `node.tag`, never an
        // attribute named `this`). FAILS against the pre-fix code that modeled `this`
        // as an `AttrIr::Dynamic { name: "this" }`.
        use super::super::ir::AttrIr;
        let alloc = Allocator::default();
        let src = "<script>let tag=$state('span');</script>\n<svelte:element this={tag} id={tag}>hi</svelte:element>";
        let ir = lower(src, &alloc);
        let special = ir
            .nodes
            .iter()
            .find_map(|n| match n {
                IrNode::Special(s) if s.kind == SpecialKind::Element => Some(s),
                _ => None,
            })
            .expect("a <svelte:element> node exists");
        // The dynamic-tag `this_expr` fact is set.
        let this_expr = special
            .this_expr
            .expect("the `this={tag}` dynamic-tag expression is captured as `this_expr`");
        assert_eq!(
            ir.analysis.expressions.get(this_expr).source.trim(),
            "tag",
            "the this_expr is the `tag` selector expression"
        );
        // The `this` attribute is NOT in the generic attribute list.
        assert!(
            !special.attrs.iter().any(|a| matches!(
                a,
                AttrIr::Dynamic { name, .. } | AttrIr::Static { name, .. } | AttrIr::Mixed { name, .. }
                    if name == "this"
            )),
            "the `this` selector is NOT a generic attribute (attrs = {:?})",
            special.attrs
        );
        // A genuine attribute (`id={tag}`) DOES stay in the attribute list.
        assert!(
            special
                .attrs
                .iter()
                .any(|a| matches!(a, AttrIr::Dynamic { name, .. } if name == "id")),
            "a real `id={{tag}}` attribute stays in the attribute list (attrs = {:?})",
            special.attrs
        );
    }
}

// ---------------------------------------------------------------------------
// H8-fix — non-rendering template constructs (`{@const}` / `{const}`/`{let}` /
// `{@debug}` / `{@attach}` / a `{#snippet}` DECLARATION) emit NO body skeleton
// content: they are not roots, not `<!>` anchors, and never shift a sibling's body
// position or trip the multi-root fragment flag. (A `{@render}` / `{@html}` / a
// block `{#if}`/`{#each}`/`{#await}`/`{#key}` DOES render — keeps its anchor.)
// Derived empirically against svelte@5.56.3 (surfaced by the H8 full-corpus matrix
// on `declaration_tags/const_tag` + `components/child_and_snippet`).
// ---------------------------------------------------------------------------

mod non_rendering_construct_excluded_from_skeleton {
    use super::*;

    fn skeletons(src: &str) -> Vec<(String, Option<String>)> {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        plan_static_templates(&ir, None)
            .templates
            .iter()
            .filter_map(|t| match t {
                TemplateFactory::FromHtml {
                    html,
                    fragment_flag,
                    ..
                } => Some((html.clone(), fragment_flag.map(|f| f.literal()))),
                TemplateFactory::TextNode { .. }
                | TemplateFactory::CommentAnchor { .. }
                | TemplateFactory::Standalone { .. } => None,
            })
            .collect()
    }

    #[test]
    fn at_const_in_each_body_emits_no_anchor() {
        // official svelte@5.56.3: an `{@const}` before `<li>` in an `{#each}` body
        // produces a body skeleton of just `<li> </li>` — NO `<!>` anchor for the
        // const. The pre-fix code emitted `<!><li></li>`.
        let src = "<script>let items=$state([1]);</script>\n<ul>{#each items as item}{@const t=item*2}<li>{t}</li>{/each}</ul>";
        let skel = skeletons(src);
        // The each-body region is `<li> </li>` (normalized `<li></li>`) — no anchor.
        assert!(
            skel.iter()
                .any(|(h, _)| h == "<li> </li>" || h == "<li></li>"),
            "the each body is just `<li>` with no `{{@const}}` anchor (got {skel:?})"
        );
        assert!(
            !skel.iter().any(|(h, _)| h.contains("<!><li")),
            "no `<!>` anchor is emitted for the `{{@const}}` (got {skel:?})"
        );
    }

    #[test]
    fn snippet_declaration_at_body_root_is_not_a_root() {
        // official: a `{#snippet}` DECLARATION at a body position renders nothing —
        // the body is `<div>x</div><ul></ul>` (the snippet declaration is invisible).
        // The pre-fix code added a spurious `<!>` body root for the snippet.
        let src = "<script>let r=$state([1]);</script>\n{#snippet row(n)}<li>row {n}</li>{/snippet}\n<div>x</div>\n<ul>{#each r as n}{@render row(n)}{/each}</ul>";
        let skel = skeletons(src);
        // The body region holds the div + ul. The snippet body `<li>` is its OWN
        // region (not in the body root).
        let body = skel
            .iter()
            .find(|(h, _)| h.contains("<div>x</div>"))
            .unwrap_or_else(|| panic!("a body region with the div exists (got {skel:?})"));
        // Exactly ONE `<!>` (the each anchor in the ul) — no snippet anchor.
        let anchor_count = body.0.matches("<!>").count();
        assert!(
            anchor_count <= 1,
            "the snippet declaration adds no extra `<!>` body anchor (got body {:?}, {anchor_count} anchors)",
            body.0
        );
        // The snippet body `<li>row …</li>` is a SEPARATE region, not in the body.
        assert!(
            !body.0.contains("<li"),
            "the snippet body `<li>` is its own region, not in the body root (got {:?})",
            body.0
        );
    }

    #[test]
    fn debug_tag_child_emits_no_anchor() {
        // A `{@debug}` is non-rendering: a `<div>` with text + a `{@debug}` child
        // stays `<div>x</div>` (the construct is invisible).
        let src = "<script>let v=$state(1);</script>\n<div>x{@debug v}</div>";
        let skel = skeletons(src);
        assert!(
            skel.iter().any(|(h, _)| h == "<div>x</div>"),
            "a `{{@debug}}` child emits no body anchor (got {skel:?})"
        );
    }
}

// ---------------------------------------------------------------------------
// G4 — static-HTML escaping. The official `$.from_html` skeleton emits TEXT
// content RAW (author entities `&amp;`/`&lt;` pass through, NOT double-escaped),
// while ATTRIBUTE values are entity-AWARE escaped (`"` → `&quot;`, bare `&` →
// `&amp;`, `<` → `&lt;`, but an already-valid `&amp;` is preserved). Derived
// empirically against svelte@5.56.3. FAILS against the pre-fix double-escaping
// `escape_html_text` (`&amp;` → `&amp;amp;`).
// ---------------------------------------------------------------------------

#[test]
fn static_text_content_is_raw_not_double_escaped() {
    let alloc = Allocator::default();
    // Author text carrying existing entities + a bare `&`. The official compiler
    // passes ALL of them through verbatim into the from_html template string.
    let src = "<div>x &amp; y &lt; z &nbsp; Tom &amp; Jerry</div>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let html = match &plan.templates[0] {
        TemplateFactory::FromHtml { html, .. } => html.clone(),
        other => panic!("expected a from_html factory, got {other:?}"),
    };
    // The author entities pass through verbatim (raw text — NOT re-escaped).
    assert!(
        html.contains("x &amp; y &lt; z &nbsp; Tom &amp; Jerry"),
        "static text content is emitted raw (author entities preserved) (got {html:?})"
    );
    // Negative: NO double-escaping — `&amp;amp;` / `&amp;lt;` / `&amp;nbsp;` must
    // not appear (the pre-fix `&`→`&amp;` first-pass bug).
    assert!(
        !html.contains("&amp;amp;"),
        "an authored `&amp;` must NOT be double-escaped to `&amp;amp;` (got {html:?})"
    );
    assert!(
        !html.contains("&amp;lt;") && !html.contains("&amp;nbsp;"),
        "authored entities must NOT be double-escaped (got {html:?})"
    );
}

#[test]
fn static_attribute_value_is_entity_aware_escaped() {
    let alloc = Allocator::default();
    // An attribute value containing a `"` (must become `&quot;` so it cannot break
    // out of the double-quoted skeleton), a bare `&` (→ `&amp;`), and an authored
    // `&amp;` (preserved, NOT doubled).
    let src = "<div title='say \"hi\" Tom & Jerry &amp; co'>x</div>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let html = match &plan.templates[0] {
        TemplateFactory::FromHtml { html, .. } => html.clone(),
        other => panic!("expected a from_html factory, got {other:?}"),
    };
    // The `"` is escaped (does not break the quoted attribute).
    assert!(
        html.contains("&quot;hi&quot;"),
        "an attribute-value `\"` is escaped to `&quot;` (got {html:?})"
    );
    // A bare `&` becomes `&amp;`, an authored `&amp;` is preserved (no doubling).
    assert!(
        html.contains("Tom &amp; Jerry &amp; co"),
        "a bare `&` is escaped and an authored `&amp;` is preserved (got {html:?})"
    );
    // Negative: no raw `"` inside the attribute value, no double-escaped entity.
    assert!(
        !html.contains("\"hi\""),
        "the raw unescaped `\"` must not appear in the attribute value (got {html:?})"
    );
    assert!(
        !html.contains("&amp;amp;"),
        "an authored `&amp;` in an attribute must NOT be double-escaped (got {html:?})"
    );
}

// ---------------------------------------------------------------------------
// H7 — attribute-value entity handling is official-complete: the official
// compiler DECODES the attribute value (named common + numeric entities) THEN
// re-escapes it for the double-quoted context (`escape_html(decoded, is_attr)`).
// A valid named entity (`&nbsp;`) decodes to its char; a numeric entity
// (`&#65;`/`&#x41;`) decodes to its code point; an INVALID entity (`&bogus;`) is
// NOT a real entity, so its leading `&` is escaped (`&amp;bogus;`). TEXT content
// stays RAW (a separate path) — only ATTRIBUTE values decode-then-reencode.
// Derived empirically against svelte@5.56.3.
// ---------------------------------------------------------------------------

mod attribute_entity_decode {
    use super::*;

    fn attr_skeleton(src: &str) -> String {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        match &plan_static_templates(&ir, None).templates[0] {
            TemplateFactory::FromHtml { html, .. } => html.clone(),
            other => panic!("expected a from_html factory, got {other:?}"),
        }
    }

    #[test]
    fn valid_named_entity_decodes_to_char() {
        // official svelte@5.56.3: `title="a&nbsp;b"` → the NBSP char (U+00A0) in the
        // skeleton (decoded), NOT the literal `&nbsp;`.
        let html = attr_skeleton("<div title=\"a&nbsp;b\">x</div>");
        assert!(
            html.contains("title=\"a\u{00a0}b\""),
            "a `&nbsp;` attribute entity decodes to the NBSP char (got {html:?})"
        );
        // Negative: the literal `&nbsp;` must NOT survive in the attribute value.
        assert!(
            !html.contains("&nbsp;"),
            "the literal `&nbsp;` must not survive (it decodes) (got {html:?})"
        );
    }

    #[test]
    fn invalid_named_entity_escapes_the_ampersand() {
        // official: `title="a&bogus;b"` → "a&amp;bogus;b" — `&bogus;` is NOT a real
        // entity, so the leading `&` is escaped. The pre-fix shape-only check
        // WRONGLY preserved `&bogus;` (treating any `&name;` as a valid entity).
        let html = attr_skeleton("<div title=\"a&bogus;b\">x</div>");
        assert!(
            html.contains("title=\"a&amp;bogus;b\""),
            "an invalid `&bogus;` attribute entity escapes the `&` to `&amp;bogus;` (got {html:?})"
        );
    }

    #[test]
    fn numeric_entities_decode_to_code_point() {
        // official: `title="a&#65;b"` and `title="a&#x41;b"` → "aAb" (code point 65).
        let dec = attr_skeleton("<div title=\"a&#65;b\">x</div>");
        assert!(
            dec.contains("title=\"aAb\""),
            "a decimal numeric entity decodes to its char (got {dec:?})"
        );
        let hex = attr_skeleton("<div title=\"a&#x41;b\">x</div>");
        assert!(
            hex.contains("title=\"aAb\""),
            "a hex numeric entity decodes to its char (got {hex:?})"
        );
    }

    #[test]
    fn valid_amp_entity_round_trips() {
        // A valid `&amp;` decodes to `&` then re-escapes to `&amp;` (round-trip, no
        // doubling). A bare `&` escapes to `&amp;`.
        let html = attr_skeleton("<div title=\"a&amp;b Tom & Jerry\">x</div>");
        assert!(
            html.contains("title=\"a&amp;b Tom &amp; Jerry\""),
            "valid `&amp;` round-trips and a bare `&` escapes (got {html:?})"
        );
        assert!(
            !html.contains("&amp;amp;"),
            "no double-escaping of a valid `&amp;` (got {html:?})"
        );
    }

    #[test]
    fn text_content_entities_stay_raw() {
        // Contrast (the separate TEXT path): TEXT content keeps `&nbsp;` / `&bogus;`
        // RAW — the decode-then-reencode is ATTRIBUTE-only.
        let html = attr_skeleton("<div>a&nbsp;b &bogus; c</div>");
        assert!(
            html.contains("a&nbsp;b &bogus; c"),
            "text content entities stay raw (got {html:?})"
        );
    }

    // U2 — the attribute decode is OFFICIAL-COMPLETE (the full HTML5 named table +
    // numeric `validate_code` incl. the Windows-1252 remap), not a hand-rolled
    // ~30-entry subset. A valid named entity OUTSIDE the old subset must DECODE to
    // its char (NOT be wrong-decoded to `&amp;<name>;`); a numeric code in
    // 128..=159 applies the Windows-1252 remap. Confirmed against svelte@5.56.3:
    // `&ouml;`→`ö`, `&notin;`→`∉`, `&#128;`→`€`, `&bogus;`→`&amp;bogus;`.

    #[test]
    fn named_entity_outside_legacy_subset_decodes_not_wrong_escaped() {
        // official: `title="&ouml;"` → the `ö` char (U+00F6), NOT `&amp;ouml;`. The
        // pre-fix subset returned None for `ouml` and WRONG-escaped the `&` to
        // `&amp;ouml;` (a WRONG DOM value — renders the literal `&ouml;`).
        let html = attr_skeleton("<div title=\"&ouml;\">x</div>");
        assert!(
            html.contains("title=\"\u{00f6}\""),
            "a `&ouml;` attribute entity decodes to `ö` (got {html:?})"
        );
        // Negative: the WRONG `&amp;ouml;` must NOT appear.
        assert!(
            !html.contains("&amp;ouml;"),
            "a valid named entity must NOT be wrong-decoded to `&amp;<name>;` (got {html:?})"
        );
    }

    #[test]
    fn mathematical_named_entity_decodes() {
        // official: `title="&notin;"` → `∉` (U+2209).
        let html = attr_skeleton("<div title=\"&notin;\">x</div>");
        assert!(
            html.contains("title=\"\u{2209}\""),
            "a `&notin;` attribute entity decodes to `∉` (got {html:?})"
        );
        assert!(
            !html.contains("&amp;notin;"),
            "`&notin;` must not be wrong-escaped (got {html:?})"
        );
    }

    #[test]
    fn numeric_windows_1252_code_is_remapped() {
        // official: `title="&#128;"` → `€` (U+20AC) — the Windows-1252 remap of code
        // point 128 (the HTML `validate_code` rule for 128..=159). The pre-fix
        // `validate_entity_code` left 128 as-is (a control char), a wrong value.
        let html = attr_skeleton("<div title=\"&#128;\">x</div>");
        assert!(
            html.contains("title=\"\u{20ac}\""),
            "a numeric `&#128;` is Windows-1252-remapped to `€` (got {html:?})"
        );
    }

    #[test]
    fn invalid_named_entity_still_escapes_the_ampersand() {
        // official (unchanged): `title="&bogus;"` → `&amp;bogus;` — `&bogus;` is NOT
        // a real entity, so its `&` is escaped. The full-table decode must NOT
        // accidentally decode a non-entity.
        let html = attr_skeleton("<div title=\"&bogus;\">x</div>");
        assert!(
            html.contains("title=\"&amp;bogus;\""),
            "an invalid `&bogus;` still escapes the `&` to `&amp;bogus;` (got {html:?})"
        );
    }

    #[test]
    fn legacy_no_semicolon_named_entity_decodes() {
        // The HTML5 table includes LEGACY no-semicolon named refs (`&copy` without
        // `;`). official svelte@5.56.3 decodes `&copy` (no `;`) to `©` in an
        // attribute value. (The decode-then-reescape leaves `©` as-is — not in the
        // `[&"<]` escape set.)
        let html = attr_skeleton("<div title=\"a&copy b\">x</div>");
        assert!(
            html.contains("\u{00a9}"),
            "a legacy no-semicolon `&copy` named entity decodes to `©` (got {html:?})"
        );
    }

    /// The attribute value of the single `from_html` skeleton (the `title="…"`
    /// inner text), for the exact-byte parity table below.
    fn attr_value(src: &str) -> String {
        let html = attr_skeleton(src);
        let start = html.find("title=\"").expect("title attr") + "title=\"".len();
        let rest = &html[start..];
        let end = rest.find('"').expect("closing quote");
        rest[..end].to_string()
    }

    #[test]
    fn attribute_entity_decode_matches_official_byte_for_byte() {
        // EXACT parity against the pinned svelte@5.56.3 compiler output for a broad
        // set of entity forms — the discriminating full-table + validate_code +
        // attribute-boundary check. Each `(input, official_attr_value)` row was
        // ground-truthed against the pinned compiler.
        let cases: &[(&str, &str)] = &[
            ("&ouml;", "\u{00f6}"),                // named outside the legacy subset → ö
            ("&notin;", "\u{2209}"),               // mathematical named → ∉
            ("&Dagger;", "\u{2021}"),              // ‡
            ("&hearts;", "\u{2665}"),              // ♥
            ("&frac12;", "\u{00bd}"),              // ½
            ("&alpha;&beta;", "\u{03b1}\u{03b2}"), // adjacent named refs → αβ
            ("&#65;", "A"),                        // decimal numeric
            ("&#x41;", "A"),                       // hex numeric
            ("&#128;", "\u{20ac}"),                // Windows-1252 remap → €
            ("&#10;x", " x"),                      // line feed → space
            ("&#x1F600;", "\u{1F600}"),            // supplementary plane → 😀
            ("&amp;", "&amp;"),                    // valid amp round-trips
            ("&lt;&gt;", "&lt;>"),                 // `<` re-escaped, `>` not in escape set
            ("&copy", "\u{00a9}"),                 // legacy no-`;` followed by quote → ©
            ("a&copy=b", "a&amp;copy=b"),          // legacy no-`;` before `=` → NOT decoded
            ("&bogus;", "&amp;bogus;"),            // unknown reference → `&` escaped
            ("&#0;", "&amp;#0;"),                  // falsy code 0 → literal kept, `&` escaped
            ("Tom & Jerry", "Tom &amp; Jerry"),    // bare `&` escaped
            // Longest-match boundary: a legacy no-`;` key that is a PREFIX of the
            // following text must NOT decode when followed by an alphanumeric.
            ("&copyright;", "&amp;copyright;"), // `copy` blocked by following `r`
            ("&ampersand;", "&amp;ampersand;"), // `amp` blocked by following `e`
            ("&lt;x", "&lt;x"),                 // `lt;` decodes `<` → `&lt;`, then `x`
            ("&ltx", "&amp;ltx"),               // `lt` (no `;`) blocked by following `x`
        ];
        for (input, expected) in cases {
            let src = format!("<div title=\"{input}\">x</div>");
            let got = attr_value(&src);
            assert_eq!(
                &got, expected,
                "attribute entity decode for {input:?} must match official byte-for-byte (got {got:?})"
            );
        }
    }

    #[test]
    fn surrogate_numeric_code_decodes_to_nul_char() {
        // A surrogate-half numeric code is a TRUTHY code that `validate_code` maps to
        // NUL (`0`) → the NUL char (official `String.fromCodePoint(0)`), DISTINCT
        // from a falsy `&#0;` (kept literal). Confirmed vs svelte@5.56.3:
        // `&#xD800;` → a single U+0000 char.
        let got = attr_value("<div title=\"&#xD800;\">x</div>");
        assert_eq!(
            got, "\u{0000}",
            "a surrogate-half numeric code decodes to the NUL char (got {got:?})"
        );
    }
}

// ---------------------------------------------------------------------------
// G7 — static-text whitespace is normalized over the sibling sequence
// (`clean_nodes`): the significant space between a text run and a nested element
// is preserved (NOT per-node `trim()`-dropped), interior whitespace within a text
// node is preserved, and leading/trailing fragment whitespace is trimmed. Derived
// empirically against svelte@5.56.3. FAILS against the per-node-trim serializer
// (`Hello <strong>` → `Hello<strong>`).
// ---------------------------------------------------------------------------

#[test]
fn static_whitespace_preserved_around_nested_element() {
    let alloc = Allocator::default();
    // svelte@5.56.3: `<p>Hello <strong>world</strong> !</p>` →
    // `<p>Hello <strong>world</strong> !</p>` — both significant spaces kept.
    let src = "<p>Hello <strong>world</strong> !</p>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let html = match &plan.templates[0] {
        TemplateFactory::FromHtml { html, .. } => html.clone(),
        other => panic!("expected a from_html factory, got {other:?}"),
    };
    assert_eq!(
        html, "<p>Hello <strong>world</strong> !</p>",
        "the significant whitespace around the nested element is preserved"
    );
    // Negative: the space before `<strong>` must NOT be dropped.
    assert!(
        !html.contains("Hello<strong>"),
        "the space before the nested element must not be trimmed away (got {html:?})"
    );
}

#[test]
fn static_interior_whitespace_preserved_but_edges_trimmed() {
    let alloc = Allocator::default();
    // svelte@5.56.3: `<p>  Hello   world  </p>` → `<p>Hello   world</p>` — the
    // leading/trailing whitespace is trimmed, but the INTERIOR run (3 spaces) is
    // preserved verbatim (NOT collapsed to one). FAILS against `collapse_text`.
    let src = "<p>  Hello   world  </p>";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let html = match &plan.templates[0] {
        TemplateFactory::FromHtml { html, .. } => html.clone(),
        other => panic!("expected a from_html factory, got {other:?}"),
    };
    assert_eq!(
        html, "<p>Hello   world</p>",
        "interior whitespace is preserved; only leading/trailing is trimmed"
    );
    assert!(
        !html.contains("Hello world"),
        "the interior 3-space run must NOT be collapsed to one space (got {html:?})"
    );
}

// ---------------------------------------------------------------------------
// U5 — HTML-significance / root filtering uses the ASCII `is_html_ws` set, NOT
// Rust `trim()` / `char::is_whitespace` (which fold a literal NBSP `\u{00a0}` and
// other Unicode whitespace). A literal-NBSP text node is SIGNIFICANT content (the
// official `clean_nodes` only drops ASCII ` \t\r\n`), so it must NOT be dropped
// from the effective-root sequence as "whitespace-only". Derived empirically
// against svelte@5.56.3. FAILS against the `trim()`-based `is_whitespace_text`.
// ---------------------------------------------------------------------------

#[test]
fn literal_nbsp_root_is_significant_not_whitespace_dropped() {
    let alloc = Allocator::default();
    // svelte@5.56.3: `\u{00a0}{#if …}<p>x</p>{/if}` lowers to TWO regions —
    // `<p>x</p>` (the if-branch body) and ` <!>` (the leading NBSP text + the block
    // anchor). The leading NBSP is SIGNIFICANT (not ASCII whitespace), so the root
    // sequence keeps the NBSP text node as an effective root, yielding a multi-root
    // (fragment-flagged) `from_html` skeleton that embeds the NBSP — NOT a single
    // `<p>x</p>` region with the NBSP silently dropped.
    let src = "\u{00a0}{#if true}<p>x</p>{/if}";
    let ir = lower(src, &alloc);
    let plan = plan_static_templates(&ir, None);
    let htmls: Vec<String> = plan
        .templates
        .iter()
        .filter_map(|t| match t {
            TemplateFactory::FromHtml { html, .. } => Some(html.clone()),
            TemplateFactory::TextNode { .. }
            | TemplateFactory::CommentAnchor { .. }
            | TemplateFactory::Standalone { .. } => None,
        })
        .collect();
    // The root region embeds the literal NBSP (`\u{00a0}<!>`) — it was NOT folded
    // away as whitespace. A `trim()`-based filter would drop the NBSP root and emit
    // only `<p>x</p>` (the bug).
    assert!(
        htmls.iter().any(|h| h.contains('\u{00a0}')),
        "the literal NBSP root text must survive into the skeleton (not whitespace-dropped); got {htmls:?}"
    );
    // The NBSP-bearing root region carries the block anchor marker (the NBSP is a
    // root SIBLING of the block, so the root is multi-root with the `<!>` anchor).
    assert!(
        htmls
            .iter()
            .any(|h| h.contains('\u{00a0}') && h.contains("<!>")),
        "the NBSP root sibling sits beside the block comment anchor; got {htmls:?}"
    );
    // Negative: the NBSP root must NOT be folded away leaving only the if-branch
    // body `<p>x</p>` (the `trim()`-based-filter bug).
    assert!(
        htmls != vec!["<p>x</p>".to_string()],
        "the NBSP root must not be dropped, leaving only the if-branch body; got {htmls:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 10 — topology summary diffed against the conformance oracle goldens on the
// IR-determinable axes (structural-helper SUBSET membership + exact template
// skeleton + ImportPlan + DelegatedEvents). Does NOT require official helper
// SEQUENCE byte-order parity (that is the emitting backend's gate); the planner's
// OWN recorded sequence is asserted to be in IR-traversal order separately.
// ---------------------------------------------------------------------------

mod topology_oracle {
    use super::*;
    use serde::Deserialize;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[derive(Debug, Deserialize)]
    struct GoldenImport {
        source: String,
        kind: String,
        #[allow(dead_code)]
        names: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct GoldenTemplate {
        #[allow(dead_code)]
        factory: String,
        html: String,
        flag: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct Golden {
        slug: String,
        #[serde(rename = "helperSet")]
        helper_set: Vec<String>,
        /// The official ORDERED delegated event-type set (the module
        /// `$.delegate([...])` declaration), client backend only.
        #[serde(rename = "delegatedEvents", default)]
        delegated_events: Vec<String>,
        imports: Vec<GoldenImport>,
        templates: Vec<GoldenTemplate>,
    }

    /// The COMPLETE set of structural runtime helpers the pre-lowering plan can
    /// ever record — its OWNED universe. EXCLUDES the fine-grained DOM-WALK helpers
    /// (`first_child` / `child` / `sibling` / `reset` / `next`) and the script
    /// read-rewrite helpers (`get` / `set` / `state` / `proxy` / `template_effect` /
    /// `set_text`) — those are the emitting backend's concern. The text-first ROOT
    /// factory `$.text` IS owned (it is a region's mount root, structurally parallel
    /// to `from_html` / `comment`, recorded per text-first region in
    /// [`plan_client_topology`]); only the INTERIOR reactive `$.text()` nodes a
    /// `from_html` region creates mid-walk stay the backend's concern.
    ///
    /// Every helper the topology planner calls MUST appear here, so the topology
    /// assertion can pin the planned set EXACTLY (membership) against the
    /// intersection of the official helper set with this universe — no subset / `<=`
    /// slack remains.
    const OWNED_STRUCTURAL_HELPERS: &[SvelteHelper] = &[
        SvelteHelper::FromHtml,
        SvelteHelper::FromTree,
        SvelteHelper::Text,
        SvelteHelper::Comment,
        SvelteHelper::Append,
        SvelteHelper::If,
        SvelteHelper::Each,
        SvelteHelper::Await,
        SvelteHelper::Key,
        SvelteHelper::Html,
        SvelteHelper::Snippet,
        SvelteHelper::Slot,
        SvelteHelper::Delegated,
        SvelteHelper::Event,
        SvelteHelper::Delegate,
        SvelteHelper::Head,
        SvelteHelper::BindThis,
        SvelteHelper::BindValue,
        // The 5c DOM-hosted bind family + the textarea prelude.
        SvelteHelper::BindSelectValue,
        SvelteHelper::BindChecked,
        SvelteHelper::BindGroup,
        SvelteHelper::BindCurrentTime,
        SvelteHelper::BindPaused,
        SvelteHelper::BindPlayed,
        SvelteHelper::BindElementSize,
        SvelteHelper::BindContentEditable,
        SvelteHelper::BindProperty,
        // The 5f-b special-host bind helpers (`<svelte:window|document>`).
        SvelteHelper::BindWindowSize,
        SvelteHelper::BindWindowScroll,
        SvelteHelper::BindOnline,
        SvelteHelper::BindFocused,
        SvelteHelper::BindActiveElement,
        SvelteHelper::AttributeEffect,
    ];

    fn goldens_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_oracle_corpus/goldens")
    }

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_oracle_corpus/fixtures")
    }

    fn load_golden(rel: &str) -> Golden {
        let path = goldens_dir().join(rel);
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
        serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("parse golden {}: {e}", path.display()))
    }

    fn load_fixture(slug: &str) -> String {
        let path = fixtures_dir().join(slug);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
    }

    /// The serialized template skeleton our planner produced as `(html, flag)`
    /// rows, in plan order. A comment-anchor factory is excluded (the tight
    /// fixture set has no comment-anchor root). `css` is the fixture's proven
    /// scope-injection facts (`None` for a style-less fixture) — the skeleton
    /// bake consumes it exactly as the production pipeline threads it.
    fn planner_templates(
        ir: &crate::svelte::runtime::ir::SvelteRuntimeIr,
        css: Option<&crate::svelte::runtime::css::types::CssScopeFacts>,
    ) -> Vec<(String, Option<String>)> {
        let plan = plan_static_templates(ir, css);
        plan.templates
            .iter()
            .filter_map(|t| match t {
                // A `fragments: 'tree'` factory carries its objectified array
                // literal in `tree`; the golden's template body is that same
                // literal, so the skeleton comparison must use it (not the
                // still-populated `html` skeleton string) when present.
                TemplateFactory::FromHtml {
                    html,
                    tree,
                    fragment_flag,
                    ..
                } => Some((
                    tree.clone().unwrap_or_else(|| html.clone()),
                    fragment_flag.map(|f| f.literal()),
                )),
                TemplateFactory::TextNode { .. }
                | TemplateFactory::CommentAnchor { .. }
                | TemplateFactory::Standalone { .. } => None,
            })
            .collect()
    }

    /// Build the fixture's proven css scope-injection facts from its top-level
    /// `<style>` (`None` for a style-less fixture). Mirrors the production
    /// wiring: parse + analyze + match over the runtime IR; a fixture that
    /// fails the plan build (analysis OR matcher-unprovable) fails LOUD (the
    /// vendored corpus is supported by construction).
    fn fixture_scope_facts(
        source: &str,
        ir: &crate::svelte::runtime::ir::SvelteRuntimeIr,
    ) -> Option<crate::svelte::runtime::css::types::CssScopeFacts> {
        use crate::svelte::runtime::css;
        let parsed = parse_svelte(source);
        let style = parsed.styles.first()?;
        let content = style.content.expect("a corpus style has a body span");
        // The matrix hash input is irrelevant (the skeleton comparison masks
        // every `svelte-<hash>` token), so the css-text fallback suffices; the
        // mode does not affect the skeleton.
        let plan = css::build_style_scope_plan(
            source,
            content,
            None,
            css::types::CssMode::External,
            ir,
            false,
        )
        .expect("a corpus style analyzes and proves its matcher facts");
        Some(plan.scope_facts())
    }

    /// Mask every `svelte-<hash>` scope token to the golden's `svelte-<scoped>`
    /// placeholder — the Rust port of the golden generator's `maskScopeHash`
    /// (`/svelte-[0-9a-z]+/g`), so the skeleton comparison is hash-independent.
    fn mask_scope_hash(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < bytes.len() {
            if text[i..].starts_with("svelte-") {
                let start = i + "svelte-".len();
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_digit() || bytes[end].is_ascii_lowercase())
                {
                    end += 1;
                }
                if end > start {
                    out.push_str("svelte-<scoped>");
                    i = end;
                    continue;
                }
            }
            // Advance one CHAR (the skeleton may contain multi-byte decoded
            // entities).
            let ch = text[i..].chars().next().expect("in-bounds char");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// Normalize the dynamic-text placeholder inside an otherwise-empty element
    /// for the structural skeleton comparison: a single-space-only element body
    /// (`<tag> </tag>`) and an empty body (`<tag></tag>`) both denote "an element
    /// with a dynamic text slot and no static text". Whether the runtime uses a
    /// ` ` placeholder text node or creates the text node fresh is the emitting
    /// backend's text-node strategy, so it is normalized away here.
    ///
    /// Implemented as a forward CHAR scan that drops the lone ` ` between a `>`
    /// and a `<` (an empty-element placeholder), not a string substitution and not
    /// a byte scan (the skeleton may contain non-ASCII decoded entity chars like
    /// `ö`, so the scan must stay on char boundaries).
    fn normalize_placeholder(html: &str) -> String {
        let chars: Vec<char> = html.chars().collect();
        let mut out = String::with_capacity(html.len());
        let mut i = 0;
        while i < chars.len() {
            // A `> </` run: keep the `>`, drop the single placeholder space.
            if chars[i] == '>' && i + 2 < chars.len() && chars[i + 1] == ' ' && chars[i + 2] == '<'
            {
                out.push('>');
                i += 2; // skip the space; the `<` is emitted on the next iteration
                continue;
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    /// Re-read the golden's per-helper counts (keyed for the count diff).
    fn golden_helper_counts(golden_rel: &str) -> BTreeMap<String, u32> {
        let path = goldens_dir().join(golden_rel);
        let raw = std::fs::read_to_string(&path).expect("read golden");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("parse golden");
        let mut out = BTreeMap::new();
        if let Some(counts) = value.get("helperCounts").and_then(|c| c.as_object()) {
            for (k, v) in counts {
                if let Some(n) = v.as_u64() {
                    out.insert(k.clone(), n as u32);
                }
            }
        }
        out
    }

    /// A runtime-IR-OWNED topology axis the full-corpus matrix asserts each vendored
    /// client golden against. An axis is asserted for a fixture UNLESS a
    /// [`DEFERRAL_LEDGER`] row exempts that `(fixture, axis)` pair — the ledger is
    /// the single source of truth for "what the runtime-IR substrate does not yet
    /// match official on, and which downstream layer owns it".
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TopologyAxis {
        /// The static-template skeleton (the `from_html` template strings + the
        /// multi-root fragment flag), compared as a normalized multiset.
        Skeleton,
        /// The OWNED structural-helper SET membership (intersected with the owned
        /// universe).
        OwnedHelperSet,
        /// The OWNED structural-helper per-family COUNTS.
        OwnedHelperCounts,
        /// The runtime `ImportPlan` (client namespace + disclose-version + the
        /// mode-derived legacy flag).
        ImportPlan,
        /// The node-path reachability invariants (every `PathBase::Node` is planned,
        /// every DOM-reachability slot has a path, distinct + non-empty paths).
        NodePathReachability,
        /// The delegated-event SET membership.
        DelegatedEventSet,
    }

    /// One deferral-ledger row: a `(fixture, axis)` pair the runtime-IR substrate
    /// does NOT yet match official on, the OWNING downstream layer, and WHY. The
    /// matrix skips ONLY these ledgered axes; every other axis is asserted exactly.
    /// Keep this MINIMAL — a row is justified ONLY when the divergence is a genuine
    /// downstream-layer feature (CSS scope-class injection, bind-aware input-default
    /// removal, the `from_svg` root helper, template hoisting/dedup) — an in-scope
    /// divergence is FIXED, not ledgered.
    struct LedgerRow {
        /// The fixture slug (`.svelte`).
        fixture: &'static str,
        /// The deferred axis.
        axis: TopologyAxis,
        /// The downstream layer that owns this axis (named by its capability, not a
        /// plan label).
        owning_layer: &'static str,
        /// Why the divergence is a genuine downstream-layer feature.
        reason: &'static str,
    }

    /// The full deferral ledger — the COMPLETE set of `(fixture, axis)` pairs the
    /// full-corpus topology matrix skips, each pinned to its owning downstream layer.
    /// Every divergence surfaced by the matrix that is NOT here is an in-scope bug
    /// that must be FIXED, not added here.
    const DEFERRAL_LEDGER: &[LedgerRow] = &[
        // (The `bindings/bind_checked_group.svelte` Skeleton ledger row was REMOVED:
        // 5c's bind-aware default stripping now pulls the static `value` out of a
        // `bind:group` input's skeleton, so the planned skeleton MATCHES official —
        // the matrix asserts it. The fixture as a whole still fails closed elsewhere on
        // the checkbox-group `$state([])` array-proxy, owned by 5g.)
        //
        // (The `css/scoped_styles.svelte` Skeleton ledger row was REMOVED: the
        // scope-class injection now bakes `svelte-<hash>` into the planned
        // skeleton, so the matrix asserts the fixture exactly.)
        //
        // Identical-skeleton template hoisting/dedup: official hoists two identical
        // `from_html` skeletons (the `{:then}` + `{:catch}` branches both serialize
        // to `<p></p>`) into ONE shared factory; the runtime-IR plan emits one region
        // per template scope (a reachable, semantically-equal pre-dedup form).
        LedgerRow {
            fixture: "blocks/await_block.svelte",
            axis: TopologyAxis::Skeleton,
            owning_layer: "the client emission backend (identical-template hoisting/dedup)",
            reason: "official deduplicates the identical `<p></p>` then/catch branch skeletons \
                     into one hoisted factory; the runtime-IR plan emits one region per template \
                     scope (the dedup is a backend emission optimization)",
        },
        LedgerRow {
            fixture: "blocks/await_block.svelte",
            axis: TopologyAxis::OwnedHelperCounts,
            owning_layer: "the client emission backend (identical-template hoisting/dedup)",
            reason: "same as the Skeleton row: the per-scope plan emits 3 `from_html` factories \
                     (pending + then + catch) where official's dedup hoists the identical \
                     then/catch skeletons into 2",
        },
        // A block-only each body whose sole content is a `{@render}` becomes a
        // `$.comment()` factory in the runtime-IR per-scope plan, while official
        // mounts the render through the each block's own anchor (no separate comment
        // factory). Reachable + semantically equal — the mount strategy is a backend
        // emission concern.
        //
        // svg / mathml root element emission (CATEGORY-4 POST-RELEASE deferral): a
        // non-`html` namespace is refused at the resolver and svg/mathml elements fail
        // closed, so Verter serializes an svg root as an html-namespaced `$.from_html`
        // clone where official emits the `$.from_svg` root helper. The whitespace-cleaned
        // SKELETON bytes still match (the svg-context whitespace rule is honored), so only
        // the owned helper SET / COUNTS diverge (Verter carries `from_html`; official's
        // `from_svg` is outside the owned-helper universe).
        LedgerRow {
            fixture: "whitespace/svg_whitespace.svelte",
            axis: TopologyAxis::OwnedHelperSet,
            owning_layer: "the deferred svg/mathml element-emission surface (the $.from_svg/$.from_mathml root-helper layer)",
            reason: "Verter serializes an svg root as an html-namespaced `$.from_html` clone \
                     (svg element emission is refused / deferred), so its owned helper set \
                     carries `from_html` where official emits the out-of-universe `from_svg` \
                     root helper",
        },
        LedgerRow {
            fixture: "whitespace/svg_whitespace.svelte",
            axis: TopologyAxis::OwnedHelperCounts,
            owning_layer: "the deferred svg/mathml element-emission surface (the $.from_svg/$.from_mathml root-helper layer)",
            reason: "same as the OwnedHelperSet row: the plan emits one `from_html` factory \
                     where official emits one `from_svg` (outside the owned-helper universe), \
                     so the owned `from_html` count diverges",
        },
    ];

    /// Whether `(fixture, axis)` is on the deferral ledger (and therefore skipped by
    /// the full-corpus matrix).
    fn is_ledgered(fixture: &str, axis: TopologyAxis) -> bool {
        DEFERRAL_LEDGER
            .iter()
            .any(|r| r.fixture == fixture && r.axis == axis)
    }

    /// Diff one fixture's planned topology against its committed client golden on
    /// the runtime-IR-OWNED IR-determinable axes, skipping ONLY the axes the deferral
    /// ledger exempts for this fixture.
    fn assert_fixture_topology_ledgered(slug: &str) {
        let golden_rel = slug.replace(".svelte", ".client.json");
        let golden = load_golden(&golden_rel);
        assert_eq!(
            golden.slug, slug,
            "golden identity matches the fixture slug"
        );

        let source = load_fixture(slug);
        let alloc = Allocator::default();
        let parsed = parse_svelte(&source);
        // Compile under the fixture's golden compile-options (the hand-vendored analogue
        // of `gen-svelte-goldens.mjs`'s `FIXTURE_COMPILE_OPTIONS`), threading the resolved
        // `root_options` exactly as `compile_client` does — so a `fragments: 'tree'`
        // fixture plans the `$.from_tree` factory the golden was generated with.
        let opts = fixture_runtime_options(slug);
        let resolved =
            crate::svelte::runtime::resolve_svelte_compile_options(&source, &parsed, &opts)
                .expect("fixture options resolve");
        let mut ir =
            lower_parsed_svelte_to_ir(&source, &parsed, &opts, &alloc).expect("fixture lowers");
        ir.root_options = crate::svelte::runtime::ir::RootCompileOptions {
            fragments: resolved.fragments,
            preserve_whitespace: resolved.preserve_whitespace,
            preserve_comments: resolved.preserve_comments,
            disclose_version: resolved.disclose_version,
        };
        // The fixture's proven scope-injection facts (a style-less fixture has
        // none) — the skeleton bake consumes them exactly as production does.
        let css_facts = fixture_scope_facts(&source, &ir);
        let plan = plan_static_templates(&ir, css_facts.as_ref());
        let topo = plan_client_topology(&ir, &plan, css_facts.as_ref());

        // The owned universe as a set, for membership checks.
        let owned_universe: BTreeMap<&str, ()> = OWNED_STRUCTURAL_HELPERS
            .iter()
            .map(|h| (h.ident(), ()))
            .collect();

        // (0) Universe completeness — ALWAYS asserted (never ledgered): every helper
        // the planner records MUST be in the owned universe, otherwise the
        // set/count assertions would silently miss a planner-recorded helper.
        for helper in &topo.helpers.helper_set() {
            assert!(
                owned_universe.contains_key(helper.ident()),
                "planner recorded helper `{}` (fixture {slug}) is not declared in OWNED_STRUCTURAL_HELPERS — the owned universe is incomplete",
                helper.ident()
            );
        }

        let golden_set: BTreeMap<&str, ()> =
            golden.helper_set.iter().map(|h| (h.as_str(), ())).collect();

        // (1) EXACT owned-set membership.
        if !is_ledgered(slug, TopologyAxis::OwnedHelperSet) {
            let planned_owned: std::collections::BTreeSet<&str> = topo
                .helpers
                .helper_set()
                .iter()
                .map(|h| h.ident())
                .collect();
            let expected_owned: std::collections::BTreeSet<&str> = golden_set
                .keys()
                .copied()
                .filter(|k| owned_universe.contains_key(k))
                .collect();
            assert_eq!(
                planned_owned, expected_owned,
                "the planned OWNED helper SET must EXACTLY equal the official owned-helper set for {slug}"
            );
        }

        // (2) EXACT owned-helper COUNTS.
        // LATENT CONSTRAINT (`Text`): the planner records only text-first ROOT
        // `$.text` factories (see `plan_client_topology`), so for `Text` this
        // compares a text-first-root-only planned count against the FULL golden
        // `text` count (roots PLUS interior reactive `$.text()`). It is exact today
        // only because every committed golden's interior-text count is 0; a future
        // fixture emitting an interior reactive `$.text()` must take an
        // `OwnedHelperCounts` ledger row here.
        if !is_ledgered(slug, TopologyAxis::OwnedHelperCounts) {
            let golden_counts = golden_helper_counts(&golden_rel);
            for owned in OWNED_STRUCTURAL_HELPERS {
                let planned = topo.helpers.count(*owned);
                let official = golden_counts.get(owned.ident()).copied().unwrap_or(0);
                assert_eq!(
                    planned, official,
                    "planned count for OWNED structural helper `{}` ({planned}) must EXACTLY equal the official count ({official}) for {slug}",
                    owned.ident()
                );
            }
        }

        // (3) The template skeleton (serialized static HTML + fragment flag) as a
        // normalized MULTISET.
        if !is_ledgered(slug, TopologyAxis::Skeleton) {
            // BOTH sides mask the scope hash to `svelte-<scoped>` (the golden
            // stores the masked form; Verter's planned skeleton carries the
            // real hash) — the comparison pins the scope-class TOPOLOGY, and
            // the hash VALUE is pinned by the emitted-JS/css parity gates.
            let mut planned: Vec<(String, Option<String>)> =
                planner_templates(&ir, css_facts.as_ref())
                    .into_iter()
                    .map(|(html, flag)| (normalize_placeholder(&mask_scope_hash(&html)), flag))
                    .collect();
            let mut golden_templates: Vec<(String, Option<String>)> = golden
                .templates
                .iter()
                .map(|t| (normalize_placeholder(&t.html), t.flag.clone()))
                .collect();
            planned.sort();
            golden_templates.sort();
            assert_eq!(
                planned, golden_templates,
                "the planned static-template skeleton (as a multiset) must match the golden for {slug}"
            );
        }

        // (4) ImportPlan: the client namespace + disclose-version + the mode-derived
        // legacy flag (a legacy golden carries `svelte/internal/flags/legacy`). The
        // disclose-version import is present by DEFAULT (and for every default-options
        // fixture), but the `discloseVersion: false` option fixture legitimately drops
        // it — so the planned disclose-version flag is asserted for PARITY with the
        // golden's side-effect import, never as a hard-coded `true`.
        if !is_ledgered(slug, TopologyAxis::ImportPlan) {
            assert_eq!(
                topo.imports.runtime.module_specifier(),
                "svelte/internal/client",
                "the runtime namespace is the fixed client specifier for {slug}"
            );
            let has_namespace = golden
                .imports
                .iter()
                .any(|i| i.kind == "namespace" && i.source == "svelte/internal/client");
            let has_disclose = golden
                .imports
                .iter()
                .any(|i| i.kind == "sideEffect" && i.source == "svelte/internal/disclose-version");
            assert!(
                has_namespace,
                "golden carries the client namespace import for {slug}"
            );
            assert_eq!(
                topo.imports.disclose_version, has_disclose,
                "the planned disclose-version import must match the golden's side-effect import for {slug}"
            );
            // The legacy flag must match the golden's `svelte/internal/flags/legacy`
            // side-effect import EXACTLY (H5).
            let golden_legacy = golden
                .imports
                .iter()
                .any(|i| i.kind == "sideEffect" && i.source == "svelte/internal/flags/legacy");
            assert_eq!(
                topo.imports.legacy_flag, golden_legacy,
                "the planned legacy flag must match the golden's legacy side-effect import for {slug}"
            );
        }

        // (5) The planner's OWN recorded helper sequence is in IR-traversal order:
        // the ROOT template factory is recorded FIRST. A STANDALONE root region
        // (the official `is_standalone` — a sole non-dynamic component / `{@render}`)
        // records NO template-factory helper (the component / snippet mounts against
        // the parent anchor directly), so this invariant is asserted only when the
        // plan has at least one NON-standalone factory (an actual clone / anchor).
        let has_mounting_factory = plan
            .templates
            .iter()
            .any(|f| !matches!(f, TemplateFactory::Standalone { .. }));
        if has_mounting_factory {
            assert!(
                matches!(
                    topo.helpers.sequence.first(),
                    Some(SvelteHelper::FromHtml)
                        | Some(SvelteHelper::FromTree)
                        | Some(SvelteHelper::Comment)
                        | Some(SvelteHelper::Text)
                ),
                "the planner records the template factory first (IR-traversal order) for {slug}"
            );
        }

        // (6) Node-path reachability.
        if !is_ledgered(slug, TopologyAxis::NodePathReachability) {
            // Paths are REGION-INDEXED: a node may legitimately recur across regions
            // (the same NodeId never does — ids are global — but the duplicate /
            // base-reachability invariants are now scoped PER region). Within a
            // region, a path node is unique, carries ≥1 step, and a `Node(base)` base
            // is reachable in the SAME region.
            let mut seen: Vec<(
                crate::svelte::runtime::ir::TemplateScopeId,
                crate::svelte::runtime::ir::NodeId,
            )> = Vec::new();
            for path in &plan.client_paths {
                assert!(
                    !seen.contains(&(path.scope, path.node)),
                    "client path targets a duplicate node within its region for {slug}"
                );
                seen.push((path.scope, path.node));
                assert!(
                    !path.steps.is_empty(),
                    "a client path must carry at least one walk step for {slug}"
                );
                if let crate::svelte::runtime::html::PathBase::Node(base) = path.base {
                    assert!(
                        plan.client_paths
                            .iter()
                            .any(|p| p.node == base && p.scope == path.scope),
                        "client path Node base {base:?} has no own plan in its region (unreachable) for {slug}"
                    );
                }
            }
            // Every DOM-reachability slot must have a client path IN THE SAME REGION —
            // this is the U3 nested-region completeness invariant: a Text/Html/Block
            // slot inside a nested block body must be reachable within that body's
            // own region, not merely somewhere in the (root) plan.
            for slot in &plan.slots {
                let needs_dom_path = matches!(
                    slot.kind,
                    crate::svelte::runtime::html::DynamicSlotKind::Text { .. }
                        | crate::svelte::runtime::html::DynamicSlotKind::Html { .. }
                        | crate::svelte::runtime::html::DynamicSlotKind::Block
                );
                if needs_dom_path {
                    assert!(
                        plan.client_paths
                            .iter()
                            .any(|p| p.node == slot.node && p.scope == slot.scope),
                        "dynamic slot {:?} on node {:?} (region {:?}) has no client path in its region for {slug}",
                        slot.kind,
                        slot.node,
                        slot.scope
                    );
                }
            }
        }
    }

    /// The legacy entry point used by the focused per-fixture tests below: it
    /// asserts ALL axes (these tight fixtures have no ledger rows, so the ledgered
    /// engine is byte-equivalent to the all-axes assertion for them).
    fn assert_fixture_topology(slug: &str) {
        assert_fixture_topology_ledgered(slug);
    }

    #[test]
    fn counter_topology_matches_oracle() {
        // The reactive-counter shape: a single-root reactive button with a
        // delegated onclick — the closest vendored fixture is state_primitive.
        assert_fixture_topology("runes/state_primitive.svelte");
    }

    #[test]
    fn two_root_static_template_topology_matches_oracle() {
        // A 2-root static template (one reactive, one fully static).
        assert_fixture_topology("reactive/text_interpolation.svelte");
    }

    #[test]
    fn delegated_onclick_topology_matches_oracle() {
        let slug = "events/delegated_and_native.svelte";
        assert_fixture_topology(slug);
        // The delegated fixture mixes delegated (click/input) + non-delegated
        // (mouseenter/mouseleave) events; assert the delegated set membership.
        let source = load_fixture(slug);
        let alloc = Allocator::default();
        let parsed = parse_svelte(&source);
        let ir =
            lower_parsed_svelte_to_ir(&source, &parsed, &SvelteRuntimeOptions::default(), &alloc)
                .expect("fixture lowers");
        let plan = plan_static_templates(&ir, None);
        let topo = plan_client_topology(&ir, &plan, None);
        assert!(
            topo.delegated_events.contains("click"),
            "click is delegated"
        );
        assert!(
            topo.delegated_events.contains("input"),
            "input is delegated"
        );
        assert!(
            !topo.delegated_events.contains("mouseenter"),
            "mouseenter is NOT delegated"
        );
    }

    #[test]
    fn each_block_topology_matches_oracle() {
        assert_fixture_topology("blocks/if_each_key.svelte");
    }

    #[test]
    fn static_escaping_topology_matches_oracle() {
        // G4: the escaping fixture pins the official text-raw / attribute-entity-
        // aware escaping. `assert_fixture_topology` diffs the planned static-HTML
        // skeleton against the committed golden EXACTLY (modulo the placeholder
        // normalization), so a regression in text re-escaping or attribute escaping
        // fails here against pinned official output.
        assert_fixture_topology("regression/static_escaping.svelte");
    }

    /// The HAND-VENDORED client-golden corpus (every `.svelte` fixture under
    /// `fixtures/` EXCEPT the `generated/` subtree), DISCOVERED rather than
    /// hand-listed. The full-corpus matrix asserts EVERY runtime-IR-owned axis on
    /// EVERY hand-vendored fixture except the axes the [`DEFERRAL_LEDGER`] exempts
    /// — so no in-scope divergence can hide behind a narrow hand-picked test, and a
    /// NEWLY VENDORED fixture is AUTOMATICALLY included. The slug is the fixture's
    /// path relative to `fixtures/`, `/`-joined, matching the JS golden generator's
    /// `fixtureSlug`.
    ///
    /// The `generated/` subtree is the SEPARATE differential-parity corpus (owned
    /// by `scripts/gen-svelte-diff-corpus.mjs` + the EXPANDED-schema
    /// [`generated_diff_oracle`] matrix); it is excluded here so the hand-vendored
    /// matrix's tight ledger is not polluted by the broad generated long tail.
    fn full_corpus() -> Vec<String> {
        discover_fixtures(Some(GENERATED_SUBDIR_EXCLUDE))
    }

    /// The runtime compile-options a hand-vendored fixture's golden was generated under
    /// — the Rust mirror of `gen-svelte-goldens.mjs`'s `FIXTURE_COMPILE_OPTIONS`. Only
    /// the fixtures whose golden needs a non-default option are listed; every other
    /// fixture keeps the default options. Keeps the topology matrix's planned output in
    /// sync with the option the golden pins (e.g. `fragments: 'tree'`).
    fn fixture_runtime_options(slug: &str) -> SvelteRuntimeOptions {
        let mut opts = SvelteRuntimeOptions::default();
        if slug.starts_with("options/fragments_tree_") {
            opts.fragments = Some(crate::svelte::runtime::SvelteFragments::Tree);
        }
        // The per-option EMISSION oracle fixtures (their goldens are generated with the
        // one non-default option they pin — see `gen-svelte-goldens.mjs`
        // `FIXTURE_COMPILE_OPTIONS`); compile them under that option so the planned
        // topology stays in sync with the golden the option produced.
        match slug {
            "options/preserve_comments_on.svelte" | "options/preserve_comments_multi.svelte" => {
                opts.preserve_comments = Some(true)
            }
            "options/disclose_version_off.svelte" => opts.disclose_version = Some(false),
            "options/name_reserved.svelte" => opts.name = Some("var".to_string()),
            "options/name_collision.svelte" => opts.name = Some("foo".to_string()),
            "options/name_collision_export_let.svelte"
            | "options/name_collision_snippet.svelte"
            | "options/name_collision_module_import.svelte"
            | "options/name_collision_props.svelte" => opts.name = Some("Foo".to_string()),
            "options/name_reference_collision.svelte"
            | "options/name_script_reference_collision.svelte" => {
                opts.name = Some("String".to_string())
            }
            "options/name_astral.svelte" => opts.name = Some("💩".to_string()),
            "options/namespace_svg_inline_html_wins.svelte" => {
                opts.namespace = Some(crate::svelte::runtime::SvelteNamespace::Svg)
            }
            "options/preserve_whitespace_on.svelte"
            | "options/preserve_whitespace_inline_wins.svelte" => {
                opts.preserve_whitespace = Some(true);
            }
            _ => {}
        }
        opts
    }

    /// The top-level `fixtures/` subdirectory name owned by the generated
    /// differential-parity corpus. Excluded from [`full_corpus`].
    const GENERATED_SUBDIR_EXCLUDE: &str = "generated";

    /// Discover `.svelte` fixture slugs under `fixtures/`. When `exclude_top` is
    /// `Some(name)`, the top-level subdirectory `name` is skipped; when `None`,
    /// every fixture is returned. Slugs are `/`-joined paths relative to
    /// `fixtures/`, sorted.
    fn discover_fixtures(exclude_top: Option<&str>) -> Vec<String> {
        fn walk(
            dir: &std::path::Path,
            base: &std::path::Path,
            exclude_top: Option<&str>,
            out: &mut Vec<String>,
        ) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("read corpus dir {}: {e}", dir.display()))
                .map(|e| e.expect("dir entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    // Skip the excluded top-level subdir (only at the base level).
                    if dir == base {
                        if let (Some(name), Some(file)) =
                            (exclude_top, path.file_name().and_then(|n| n.to_str()))
                        {
                            if file == name {
                                continue;
                            }
                        }
                    }
                    walk(&path, base, exclude_top, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("svelte") {
                    let rel = path
                        .strip_prefix(base)
                        .expect("fixture under base")
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    out.push(rel);
                }
            }
        }
        let base = fixtures_dir();
        let mut out = Vec::new();
        walk(&base, &base, exclude_top, &mut out);
        out.sort();
        assert!(
            !out.is_empty(),
            "the corpus fixtures/ directory must contain at least one .svelte fixture"
        );
        out
    }

    #[test]
    fn full_corpus_topology_matrix_matches_oracle() {
        // The comprehensive audit: assert EVERY runtime-IR-owned topology axis on
        // EVERY vendored client golden, skipping ONLY the ledgered axes. A divergence
        // on a NON-ledgered axis FAILS here — the matrix is the gate that surfaces any
        // latent in-scope divergence (it is what HID the module-state / non-body-
        // special / import-plan gaps behind the prior ~5-fixture test set).
        for slug in &full_corpus() {
            assert_fixture_topology_ledgered(slug);
        }
        // Emit (in the test log) what the matrix DEFERRED, so the deferral set is
        // visible at every run and no silent skip can accumulate.
        for row in DEFERRAL_LEDGER {
            eprintln!(
                "DEFERRED-AXIS {} :: {:?} → owned by {} ({})",
                row.fixture, row.axis, row.owning_layer, row.reason
            );
        }
    }

    #[test]
    fn delegated_event_set_matches_oracle_across_corpus() {
        // The delegated-event SET axis across the corpus: the planner's delegated
        // event set must EXACTLY equal (as an ordered list) the official
        // `$.delegate([...])` set the golden records — not merely be non-empty IFF
        // the golden has the `delegate` helper. This catches a wrong/missing/extra
        // delegated event (the prior non-empty-only check could not).
        for slug in &full_corpus() {
            let slug = slug.as_str();
            if is_ledgered(slug, TopologyAxis::DelegatedEventSet) {
                continue;
            }
            let golden_rel = slug.replace(".svelte", ".client.json");
            let golden = load_golden(&golden_rel);
            let source = load_fixture(slug);
            let alloc = Allocator::default();
            let parsed = parse_svelte(&source);
            let ir = lower_parsed_svelte_to_ir(
                &source,
                &parsed,
                &SvelteRuntimeOptions::default(),
                &alloc,
            )
            .expect("fixture lowers");
            let plan = plan_static_templates(&ir, None);
            let topo = plan_client_topology(&ir, &plan, None);
            // Consistency: a non-empty planned set must coincide with the golden's
            // `delegate` helper presence (the module-level set declaration).
            let golden_has_delegate = golden.helper_set.iter().any(|h| h == "delegate");
            assert_eq!(
                !golden.delegated_events.is_empty(),
                golden_has_delegate,
                "golden self-consistency: a non-empty delegatedEvents iff the `delegate` helper is present for {slug}"
            );
            // EXACT ordered-set parity against the official `$.delegate([...])`.
            let planned: Vec<String> = topo.delegated_events.ordered().to_vec();
            assert_eq!(
                planned, golden.delegated_events,
                "the planner's delegated-event set must EXACTLY equal (ordered) the official `$.delegate([...])` set for {slug}"
            );
        }
    }

    #[test]
    fn every_nested_region_interpolation_is_planned_across_corpus() {
        // U3 nested-region completeness across the WHOLE corpus: EVERY BODY-RENDERED
        // interpolation node — at the root OR inside any nested `{#if}`/`{#each}`/
        // `{#await}`/`{#key}`/snippet body — must appear in the plan as a Text slot
        // tagged with its OWNING region, with a reachable client path in that region.
        // The prior root-only collection silently lost every nested-region
        // interpolation; this matrix-wide check FAILS if any are missing.
        //
        // A `<svelte:head>` / window / … (non-body special) child interpolation is
        // EXCLUDED — those render in the special's OWN region, owned by the deferred
        // special-element region-lowering layer (see the deferral ledger), not the
        // body slot plan. The walk below mirrors `collect_node_slots`' body-reachable
        // traversal (skipping non-body-special children) so the check is consistent
        // with the slot collector's body scope, not circular with it.
        use crate::svelte::runtime::html::DynamicSlotKind;
        use crate::svelte::runtime::ir::NodeId;
        for slug in &full_corpus() {
            let slug = slug.as_str();
            let source = load_fixture(slug);
            let alloc = Allocator::default();
            let parsed = parse_svelte(&source);
            let ir = lower_parsed_svelte_to_ir(
                &source,
                &parsed,
                &SvelteRuntimeOptions::default(),
                &alloc,
            )
            .expect("fixture lowers");
            let plan = plan_static_templates(&ir, None);

            // The body-reachable interpolation node ids: walk every template scope's
            // roots, descending element/component/renderable-special children but NOT
            // a non-body special's children, and NOT into a block body (a block body
            // is reached as its OWN scope by the outer loop).
            let mut interp_nodes: Vec<NodeId> = Vec::new();
            let mut all_scopes = Vec::new();
            collect_all_template_scopes(&ir, &mut all_scopes);
            for scope_id in all_scopes {
                for &root in &ir.template_scope(scope_id).roots {
                    collect_body_interpolations(&ir, root, &mut interp_nodes);
                }
            }

            for node in interp_nodes {
                let slot = plan
                    .slots
                    .iter()
                    .find(|s| s.node == node && matches!(s.kind, DynamicSlotKind::Text { .. }));
                assert!(
                    slot.is_some(),
                    "body interpolation node {node:?} (fixture {slug}) is absent from the region-indexed slot plan — a nested-region dynamic was lost"
                );
                let slot = slot.unwrap();
                assert!(
                    plan.client_paths
                        .iter()
                        .any(|p| p.node == node && p.scope == slot.scope),
                    "body interpolation node {node:?} (fixture {slug}) has no reachable client path in its region {:?}",
                    slot.scope
                );
            }
        }
    }

    /// Collect every template-scope id (root + nested) of an IR.
    fn collect_all_template_scopes(
        ir: &super::super::ir::SvelteRuntimeIr,
        out: &mut Vec<super::super::ir::TemplateScopeId>,
    ) {
        use super::super::ir::{BlockIr, NodeId, SvelteRuntimeIr, TemplateScopeId};
        // Re-derive via the block walk: every block body / branch is a scope.
        fn walk(ir: &SvelteRuntimeIr, scope: TemplateScopeId, out: &mut Vec<TemplateScopeId>) {
            out.push(scope);
            let roots: Vec<_> = ir.template_scope(scope).roots.clone();
            for node in roots {
                walk_node(ir, node, out);
            }
        }
        fn walk_node(ir: &SvelteRuntimeIr, node: NodeId, out: &mut Vec<TemplateScopeId>) {
            match ir.node(node) {
                IrNode::Element(el) => el.children.iter().for_each(|&c| walk_node(ir, c, out)),
                IrNode::Component(c) => c.children.iter().for_each(|&c| walk_node(ir, c, out)),
                IrNode::Special(s) => s.children.iter().for_each(|&c| walk_node(ir, c, out)),
                IrNode::Block(b) => match b {
                    BlockIr::If { branches } => {
                        branches.iter().for_each(|br| walk(ir, br.body, out))
                    }
                    BlockIr::Each {
                        body, else_body, ..
                    } => {
                        walk(ir, *body, out);
                        if let Some(eb) = else_body {
                            walk(ir, *eb, out);
                        }
                    }
                    BlockIr::Await {
                        pending,
                        then_body,
                        catch_body,
                        ..
                    } => {
                        for ts in [pending, then_body, catch_body].into_iter().flatten() {
                            walk(ir, *ts, out);
                        }
                    }
                    BlockIr::Key { body, .. } => walk(ir, *body, out),
                    BlockIr::Snippet { body, .. } => walk(ir, *body, out),
                },
                _ => {}
            }
        }
        walk(ir, ir.root, out);
    }

    /// Collect the BODY-reachable interpolation node ids under `node` (descending
    /// elements/components/renderable-specials, skipping a non-body special's
    /// children and NOT descending into a block body — a block body is its own
    /// scope). Mirrors `collect_node_slots`' body traversal.
    fn collect_body_interpolations(
        ir: &super::super::ir::SvelteRuntimeIr,
        node: super::super::ir::NodeId,
        out: &mut Vec<super::super::ir::NodeId>,
    ) {
        use crate::svelte::runtime::ir::SpecialKind;
        match ir.node(node) {
            IrNode::Interpolation { .. } => out.push(node),
            IrNode::Element(el) => el
                .children
                .iter()
                .for_each(|&c| collect_body_interpolations(ir, c, out)),
            IrNode::Component(c) => c
                .children
                .iter()
                .for_each(|&c| collect_body_interpolations(ir, c, out)),
            IrNode::Special(s) => {
                // A non-body special (head / window / document / body) renders its
                // children in its OWN region — exclude them from the body check.
                let non_body = matches!(
                    s.kind,
                    SpecialKind::Head
                        | SpecialKind::Window
                        | SpecialKind::Document
                        | SpecialKind::Body
                );
                if !non_body {
                    s.children
                        .iter()
                        .for_each(|&c| collect_body_interpolations(ir, c, out));
                }
            }
            // A block node's body is a separate scope (walked by the outer loop) — do
            // not descend here.
            _ => {}
        }
    }

    /// Whether the planner's value for `(slug, axis)` ACTUALLY diverges from the
    /// official golden on that axis — the INVERSE of the matrix's per-axis
    /// assertion. Used to prove every deferral-ledger row still characterizes a
    /// REAL mismatch (a row whose axis no longer diverges is stale and must be
    /// removed). Mirrors the exact comparison
    /// [`assert_fixture_topology_ledgered`] applies for each axis.
    fn axis_diverges(slug: &str, axis: TopologyAxis) -> bool {
        let golden_rel = slug.replace(".svelte", ".client.json");
        let golden = load_golden(&golden_rel);
        let source = load_fixture(slug);
        let alloc = Allocator::default();
        let parsed = parse_svelte(&source);
        let ir =
            lower_parsed_svelte_to_ir(&source, &parsed, &SvelteRuntimeOptions::default(), &alloc)
                .expect("fixture lowers");
        let css_facts = fixture_scope_facts(&source, &ir);
        let plan = plan_static_templates(&ir, css_facts.as_ref());
        let topo = plan_client_topology(&ir, &plan, css_facts.as_ref());
        let owned_universe: BTreeMap<&str, ()> = OWNED_STRUCTURAL_HELPERS
            .iter()
            .map(|h| (h.ident(), ()))
            .collect();

        match axis {
            TopologyAxis::OwnedHelperSet => {
                let planned: std::collections::BTreeSet<&str> = topo
                    .helpers
                    .helper_set()
                    .iter()
                    .map(|h| h.ident())
                    .collect();
                let expected: std::collections::BTreeSet<&str> = golden
                    .helper_set
                    .iter()
                    .map(|h| h.as_str())
                    .filter(|k| owned_universe.contains_key(k))
                    .collect();
                planned != expected
            }
            TopologyAxis::OwnedHelperCounts => {
                let golden_counts = golden_helper_counts(&golden_rel);
                OWNED_STRUCTURAL_HELPERS.iter().any(|owned| {
                    let planned = topo.helpers.count(*owned);
                    let official = golden_counts.get(owned.ident()).copied().unwrap_or(0);
                    planned != official
                })
            }
            TopologyAxis::Skeleton => {
                let mut planned: Vec<(String, Option<String>)> =
                    planner_templates(&ir, css_facts.as_ref())
                        .into_iter()
                        .map(|(html, flag)| (normalize_placeholder(&mask_scope_hash(&html)), flag))
                        .collect();
                let mut golden_templates: Vec<(String, Option<String>)> = golden
                    .templates
                    .iter()
                    .map(|t| (normalize_placeholder(&t.html), t.flag.clone()))
                    .collect();
                planned.sort();
                golden_templates.sort();
                planned != golden_templates
            }
            TopologyAxis::ImportPlan => {
                let golden_legacy = golden
                    .imports
                    .iter()
                    .any(|i| i.kind == "sideEffect" && i.source == "svelte/internal/flags/legacy");
                topo.imports.legacy_flag != golden_legacy
            }
            TopologyAxis::DelegatedEventSet => {
                topo.delegated_events.ordered() != golden.delegated_events.as_slice()
            }
            TopologyAxis::NodePathReachability => {
                // Reachability is an INTERNAL invariant (every Node-base path has its
                // own plan; every DOM-reachable slot has a path), not a golden diff —
                // the planner always satisfies it, so it never "diverges" from a
                // golden. A reachability ledger row is therefore never justified.
                false
            }
        }
    }

    #[test]
    fn deferral_ledger_rows_are_justified_and_real() {
        // Ledger integrity: every ledger row must reference a REAL corpus fixture and
        // carry a non-empty owning-layer + reason.
        let corpus = full_corpus();
        for row in DEFERRAL_LEDGER {
            assert!(
                corpus.iter().any(|s| s == row.fixture),
                "deferral-ledger row references `{}` which is not in the corpus",
                row.fixture
            );
            assert!(
                !row.owning_layer.is_empty() && !row.reason.is_empty(),
                "deferral-ledger row for `{}` must name an owning layer and a reason",
                row.fixture
            );
            // The row must characterize a REAL, CURRENT divergence: compute the axis
            // and REQUIRE the planner to actually mismatch official while the row
            // exists. A stale row (its divergence FIXED in-scope, or never real) is
            // caught here and forces ledger removal — the ledger cannot silently skip
            // an axis that now passes.
            assert!(
                axis_diverges(row.fixture, row.axis),
                "stale deferral-ledger row: `{}` :: {:?} no longer diverges from official \
                 (the planner now MATCHES on this axis) — remove the ledger row so the matrix \
                 asserts it",
                row.fixture,
                row.axis
            );
        }
    }
}

// ---------------------------------------------------------------------------
// U3 — slots + client paths are REGION-INDEXED: every template scope (the root
// plus each nested `{#if}`/`{#each}`/`{#await}`/`{#key}`/snippet body) gets its
// OWN dynamic-slot list + node-path plan. A dynamic interpolation / bind inside a
// nested block body must appear in the plan with a reachable path — the prior
// root-only collection silently lost ALL nested-region dynamic metadata, so the
// downstream client + SSR backends could not find or update content inside a
// block body. Each slot/path carries its owning `TemplateScopeId`; the per-region
// `PathBase::Fragment` refers to that region's own cloned fragment.
// ---------------------------------------------------------------------------

mod nested_region_slot_path_completeness {
    use super::*;
    use crate::svelte::runtime::html::DynamicSlotKind;
    use crate::svelte::runtime::ir::{BlockIr, ExprId, TemplateScopeId};

    /// The body template-scope id of the first `{#each}` block in the IR.
    fn each_body_scope(ir: &super::super::ir::SvelteRuntimeIr) -> Option<TemplateScopeId> {
        ir.nodes.iter().find_map(|n| match n {
            IrNode::Block(BlockIr::Each { body, .. }) => Some(*body),
            _ => None,
        })
    }

    /// The body template-scope ids of every `{#if}` branch in the IR.
    fn if_branch_scopes(ir: &super::super::ir::SvelteRuntimeIr) -> Vec<TemplateScopeId> {
        ir.nodes
            .iter()
            .filter_map(|n| match n {
                IrNode::Block(BlockIr::If { branches }) => {
                    Some(branches.iter().map(|b| b.body).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// The source text of an interpolation expression by `ExprId`.
    fn expr_src<'a>(ir: &'a super::super::ir::SvelteRuntimeIr, id: ExprId) -> &'a str {
        ir.analysis.expressions.get(id).source
    }

    #[test]
    fn nested_each_body_interpolation_has_slot_and_reachable_path() {
        let alloc = Allocator::default();
        // `{#each items as x}<p>{x.name}</p>{/each}` — the `{x.name}` interpolation
        // lives INSIDE the each body (a nested template scope). It must surface as a
        // Text slot AND carry a reachable client path, both tagged with the each
        // body's scope id. The prior root-only collection produced NEITHER.
        let src = "<script>\n\tlet items = $state([{ name: 'a' }]);\n</script>\n{#each items as x}<p>{x.name}</p>{/each}";
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        let body = each_body_scope(&ir).expect("the each block has a body scope");

        // (1) A Text slot for `{x.name}` exists in the each-body region.
        let text_slot = plan.slots.iter().find(|s| {
            s.scope == body
                && matches!(&s.kind, DynamicSlotKind::Text { expr, .. } if expr_src(&ir, *expr).contains("x.name"))
        });
        assert!(
            text_slot.is_some(),
            "the nested each-body `{{x.name}}` interpolation must produce a Text slot tagged with the each-body scope; slots = {:?}",
            plan.slots
        );
        let text_slot = text_slot.unwrap();

        // (2) That slot's node has a reachable client path in the SAME region (the
        // path is rooted at the region's own Fragment, with at least one step).
        let path = plan
            .client_paths
            .iter()
            .find(|p| p.node == text_slot.node && p.scope == body);
        assert!(
            path.is_some(),
            "the nested each-body interpolation node must have a reachable client path in the each-body region; paths = {:?}",
            plan.client_paths
        );
        assert!(
            !path.unwrap().steps.is_empty(),
            "the nested-region path must carry at least one walk step"
        );

        // Negative: the root region must NOT (wrongly) claim the nested interpolation
        // as one of its own slots (it belongs to the each-body region).
        let root_scope_id = ir.root;
        assert!(
            !plan.slots.iter().any(|s| s.scope == root_scope_id
                && matches!(&s.kind, DynamicSlotKind::Text { expr, .. } if expr_src(&ir, *expr).contains("x.name"))),
            "the nested interpolation must be tagged with the each-body scope, NOT the root scope"
        );
    }

    #[test]
    fn nested_if_branch_dynamic_attr_has_slot_and_path() {
        let alloc = Allocator::default();
        // A dynamic ATTRIBUTE inside an `{#if}` branch body where the dynamic element
        // is NOT the sole clone-root (`<a href={url}>link</a><b>x</b>` — TWO branch
        // roots, so the branch region clones a fragment) must surface as an Attribute
        // slot AND a reachable DOM-walk path (the `<a>` is reached via
        // `first_child(fragment)`).
        let src = "<script>\n\tlet show = $state(true);\n\tlet url = $state('x');\n</script>\n{#if show}<a href={url}>link</a><b>x</b>{/if}";
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        let branches = if_branch_scopes(&ir);
        assert!(!branches.is_empty(), "the if block has at least one branch");

        // The Attribute slot for `href={url}` exists in SOME if-branch region.
        let attr_slot = plan.slots.iter().find(|s| {
            branches.contains(&s.scope)
                && matches!(&s.kind, DynamicSlotKind::Attribute { name, .. } if name == "href")
        });
        assert!(
            attr_slot.is_some(),
            "the nested if-branch `href={{url}}` dynamic attribute must produce an Attribute slot in the branch region; slots = {:?}",
            plan.slots
        );
        let attr_slot = attr_slot.unwrap();
        // The slot's element node has a reachable path in the same branch region (the
        // multi-root fragment is descended to reach the `<a>`).
        assert!(
            plan.client_paths
                .iter()
                .any(|p| p.node == attr_slot.node && p.scope == attr_slot.scope),
            "the nested if-branch dynamic-attr element must have a reachable path in the branch region; paths = {:?}",
            plan.client_paths
        );
    }

    #[test]
    fn single_root_clone_root_dynamic_attr_in_if_branch_has_no_walk() {
        let alloc = Allocator::default();
        // A dynamic ATTRIBUTE on the SOLE root element of an `{#if}` branch body
        // (`<a href={url}>link</a>`) still surfaces as an Attribute slot, but the
        // element is the branch region's CLONE-ROOT (official `is_single_element`) —
        // so it carries NO DOM-walk path (the clone var IS the element). FAILS
        // against the pre-fix code that planned a spurious `first_child` walk.
        let src = "<script>\n\tlet show = $state(true);\n\tlet url = $state('x');\n</script>\n{#if show}<a href={url}>link</a>{/if}";
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        let branches = if_branch_scopes(&ir);
        let attr_slot = plan
            .slots
            .iter()
            .find(|s| {
                branches.contains(&s.scope)
                    && matches!(&s.kind, DynamicSlotKind::Attribute { name, .. } if name == "href")
            })
            .expect("the single-root if-branch dynamic attr still produces an Attribute slot");
        // The clone-root element carries NO DOM-walk path of its own.
        assert!(
            plan.client_paths.iter().all(|p| p.node != attr_slot.node),
            "the single-root clone-root branch element carries NO DOM-walk path (it is the clone var); paths = {:?}",
            plan.client_paths
        );
    }

    #[test]
    fn deeply_nested_await_then_interpolation_is_planned() {
        let alloc = Allocator::default();
        // A `{#await p then v}{v.x}{/await}` then-branch interpolation must be
        // planned in the then-branch region (a region two levels from the root when
        // wrapped in an `{#if}`).
        let src = "<script>\n\tlet p = $state(Promise.resolve({ x: 1 }));\n\tlet show = $state(true);\n</script>\n{#if show}{#await p then v}<p>{v.x}</p>{/await}{/if}";
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        // Find the then-branch interpolation slot anywhere in the plan.
        let then_slot = plan.slots.iter().find(|s| {
            matches!(&s.kind, DynamicSlotKind::Text { expr, .. } if expr_src(&ir, *expr).contains("v.x"))
        });
        assert!(
            then_slot.is_some(),
            "the deeply-nested await-then `{{v.x}}` interpolation must be planned (region-indexed); slots = {:?}",
            plan.slots
        );
        let then_slot = then_slot.unwrap();
        // It has a reachable path in its own region.
        assert!(
            plan.client_paths
                .iter()
                .any(|p| p.node == then_slot.node && p.scope == then_slot.scope),
            "the deeply-nested await-then interpolation must have a reachable path in its region; paths = {:?}",
            plan.client_paths
        );
    }

    #[test]
    fn root_region_slots_are_tagged_with_the_root_scope() {
        let alloc = Allocator::default();
        // A root-region interpolation stays tagged with the root scope (the region
        // indexing does not mis-attribute root content).
        let src = "<script>\n\tlet n = $state(0);\n</script>\n<p>{n}</p>";
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        assert!(
            plan.slots
                .iter()
                .any(|s| s.scope == ir.root && matches!(s.kind, DynamicSlotKind::Text { .. })),
            "a root-region interpolation slot is tagged with the root scope id; slots = {:?}",
            plan.slots
        );
    }
}

// ---------------------------------------------------------------------------
// H1 — mixed nested-element + interpolation skeleton (the official
// `flush_sequence` partition): a text-run ADJACENT to an interpolation is
// dynamic text (dropped from the static skeleton + collapsed to a single ` `
// placeholder), never emitted as literal static text alongside a separate
// interpolation slot. Pinned to the pinned `svelte@5.56.3` compiler output.
// ---------------------------------------------------------------------------

mod mixed_element_interp_skeleton {
    use super::*;

    /// The single serialized `from_html` skeleton for a single-region fixture.
    fn single_skeleton(src: &str) -> String {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        let mut htmls: Vec<String> = plan
            .templates
            .iter()
            .filter_map(|t| match t {
                TemplateFactory::FromHtml { html, .. } => Some(html.clone()),
                TemplateFactory::TextNode { .. }
                | TemplateFactory::CommentAnchor { .. }
                | TemplateFactory::Standalone { .. } => None,
            })
            .collect();
        assert_eq!(
            htmls.len(),
            1,
            "expected a single from_html region for {src}"
        );
        htmls.pop().unwrap()
    }

    #[test]
    fn mixed_nested_element_with_trailing_text_interp_run() {
        // official svelte@5.56.3: `<p>Hello <b>x</b> {name}!</p>` →
        // "<p>Hello <b>x</b> </p>" — the trailing `{name}!` text+interp run is a
        // single dynamic-text placeholder, NOT `Hello <b>x</b> {placeholder}!`.
        assert_eq!(
            single_skeleton("<p>Hello <b>x</b> {name}!</p>"),
            "<p>Hello <b>x</b> </p>"
        );
    }

    #[test]
    fn mixed_leading_text_interp_run_then_element_and_trailing_text() {
        // official: `<p>{name} <b>x</b> tail</p>` → "<p> <b>x</b> tail</p>" — the
        // leading `{name} ` run collapses to one ` `, the trailing `tail` is a
        // pure-text run kept as static text.
        assert_eq!(
            single_skeleton("<p>{name} <b>x</b> tail</p>"),
            "<p> <b>x</b> tail</p>"
        );
    }

    #[test]
    fn mixed_text_interp_run_between_two_elements() {
        // official: `<p>a <b>x</b> {n} c <i>y</i></p>` → "<p>a <b>x</b> <i>y</i></p>"
        // — the `{n} c ` run between the two elements collapses to one ` `; the
        // leading `a ` pure-text run is kept.
        assert_eq!(
            single_skeleton("<p>a <b>x</b> {n} c <i>y</i></p>"),
            "<p>a <b>x</b> <i>y</i></p>"
        );
    }

    #[test]
    fn element_then_bare_interp_run() {
        // official: `<p><b>x</b>{name}</p>` → "<p><b>x</b> </p>".
        assert_eq!(single_skeleton("<p><b>x</b>{name}</p>"), "<p><b>x</b> </p>");
    }

    #[test]
    fn interp_runs_either_side_of_element() {
        // official: `<p>{a}<b>x</b>{c}</p>` → "<p> <b>x</b> </p>".
        assert_eq!(
            single_skeleton("<p>{a}<b>x</b>{c}</p>"),
            "<p> <b>x</b> </p>"
        );
    }

    #[test]
    fn two_interps_in_one_run_collapse_to_single_placeholder() {
        // official: `<p>a {m} {n} b <i>y</i></p>` → "<p> <i>y</i></p>" — the WHOLE
        // `a {m} {n} b ` run (two interps + interior text) is one placeholder.
        assert_eq!(
            single_skeleton("<p>a {m} {n} b <i>y</i></p>"),
            "<p> <i>y</i></p>"
        );
    }

    #[test]
    fn pure_static_text_around_element_is_preserved() {
        // No interpolation: `<p>a <b>x</b> b</p>` → "<p>a <b>x</b> b</p>" — both
        // pure-text runs survive (regression guard the run partition does not over-
        // drop static text).
        assert_eq!(
            single_skeleton("<p>a <b>x</b> b</p>"),
            "<p>a <b>x</b> b</p>"
        );
    }

    /// The single serialized skeleton for a fixture's element-rooted region. The
    /// fixture must produce exactly ONE element-rooted from_html region (the
    /// containing element); the block-body regions are filtered out by matching the
    /// containing-element tag prefix.
    fn element_region_skeleton(src: &str, tag_prefix: &str) -> String {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        let mut hits: Vec<String> = plan
            .templates
            .iter()
            .filter_map(|t| match t {
                TemplateFactory::FromHtml { html, .. } if html.starts_with(tag_prefix) => {
                    Some(html.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "expected one region starting with {tag_prefix} for {src}"
        );
        hits.pop().unwrap()
    }

    const STATE_PRELUDE: &str = "<script>let items=$state([1]); let x=$state(0);</script>";

    #[test]
    fn each_block_sole_child_is_controlled_no_anchor() {
        // official svelte@5.56.3: an `{#each}` that is the SOLE (whitespace-trimmed)
        // child of an element is CONTROLLED — no `<!>` anchor in the skeleton.
        // `<ul>{#each}…{/each}</ul>` → "<ul></ul>".
        let src = format!("{STATE_PRELUDE}<ul>{{#each items as i}}<li>{{i}}</li>{{/each}}</ul>");
        assert_eq!(element_region_skeleton(&src, "<ul>"), "<ul></ul>");
    }

    #[test]
    fn html_tag_sole_child_is_controlled_no_anchor() {
        // official: an `{@html}` that is the SOLE child of an element is CONTROLLED.
        // `<div>{@html x}</div>` → "<div></div>".
        let src = format!("{STATE_PRELUDE}<div>{{@html x}}</div>");
        assert_eq!(element_region_skeleton(&src, "<div>"), "<div></div>");
    }

    #[test]
    fn if_block_sole_child_is_not_controlled_keeps_anchor() {
        // official: an `{#if}` is NOT controlled even as the sole child — it keeps
        // its `<!>` anchor. `<div>{#if x}<p>a</p>{/if}</div>` → "<div><!></div>".
        let src = format!("{STATE_PRELUDE}<div>{{#if x}}<p>a</p>{{/if}}</div>");
        assert_eq!(element_region_skeleton(&src, "<div>"), "<div><!></div>");
    }

    #[test]
    fn each_block_not_sole_child_keeps_anchor() {
        // official: an `{#each}` preceded by static content is NOT controlled —
        // it keeps its `<!>` anchor. `<ul><b>h</b>{#each}…{/each}</ul>` →
        // "<ul><b>h</b><!></ul>".
        let src =
            format!("{STATE_PRELUDE}<ul><b>h</b>{{#each items as i}}<li>{{i}}</li>{{/each}}</ul>");
        assert_eq!(
            element_region_skeleton(&src, "<ul>"),
            "<ul><b>h</b><!></ul>"
        );
    }
}

// ---------------------------------------------------------------------------
// X6 / X9 — the `can_remove_entirely` whitespace removal + the `<pre>`
// first-newline discard (the official `clean_nodes` branches). A whitespace-only
// interior text node is REMOVED ENTIRELY (not collapsed to a single space) inside
// the table-family parents (select/tr/table/tbody/thead/tfoot/colgroup/datalist)
// and in an SVG context outside a `<text>` element; a `<pre>`'s leading exact-`\n`
// is discarded. All EMPIRICALLY pinned to svelte@5.56.3.
// ---------------------------------------------------------------------------

mod whitespace_removal_and_pre_newline {
    use super::*;

    /// The single serialized skeleton for a single-region fixture (a `from_html`
    /// region only). Panics if the fixture is not a single `from_html` region.
    fn single_skeleton(src: &str) -> String {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        let mut htmls: Vec<String> = plan
            .templates
            .iter()
            .filter_map(|t| match t {
                TemplateFactory::FromHtml { html, .. } => Some(html.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            htmls.len(),
            1,
            "expected a single from_html region for {src}"
        );
        htmls.pop().unwrap()
    }

    #[test]
    fn can_remove_entirely_table_family_drops_interior_whitespace() {
        // X6 — the table-family arm: a whitespace-only text between rows/options is
        // REMOVED ENTIRELY (no single-space placeholder). Each row is EMPIRICALLY
        // confirmed against svelte@5.56.3 (the interior `\n` collapses to nothing).
        let cases: &[(&str, &str)] = &[
            (
                "<select>\n<option>a</option>\n<option>b</option>\n</select>",
                "<select><option>a</option><option>b</option></select>",
            ),
            (
                "<table>\n<tbody>\n<tr>\n<td>a</td>\n</tr>\n</tbody>\n</table>",
                "<table><tbody><tr><td>a</td></tr></tbody></table>",
            ),
            (
                "<table><colgroup>\n<col>\n<col>\n</colgroup></table>",
                "<table><colgroup><col/><col/></colgroup></table>",
            ),
            (
                "<datalist>\n<option>a</option>\n<option>b</option>\n</datalist>",
                "<datalist><option>a</option><option>b</option></datalist>",
            ),
            (
                "<table><thead>\n<tr><th>h</th></tr>\n</thead><tfoot>\n<tr><td>f</td></tr>\n</tfoot></table>",
                "<table><thead><tr><th>h</th></tr></thead><tfoot><tr><td>f</td></tr></tfoot></table>",
            ),
        ];
        for (src, expected) in cases {
            assert_eq!(
                &single_skeleton(src),
                expected,
                "table-family whitespace must be removed entirely for {src}"
            );
        }
    }

    #[test]
    fn plain_div_keeps_single_space_between_elements() {
        // X6 (negative control) — a NON-table parent keeps a single space between
        // its element children (the `can_remove_entirely` arm does NOT fire).
        // svelte@5.56.3: `<div>\n<span>a</span>\n<span>b</span>\n</div>` →
        // `<div><span>a</span> <span>b</span></div>`.
        assert_eq!(
            single_skeleton("<div>\n<span>a</span>\n<span>b</span>\n</div>"),
            "<div><span>a</span> <span>b</span></div>"
        );
    }

    #[test]
    fn svg_context_drops_interior_whitespace() {
        // X6 — the SVG arm: a whitespace-only text inside an `<svg>` (outside a
        // `<text>` element) is removed entirely. svelte@5.56.3:
        // `<svg>\n<rect/>\n<circle/>\n</svg>` → `<svg><rect></rect><circle></circle></svg>`.
        // (NOTE: official uses `from_svg`; this runtime serializer's skeleton bytes
        // are the same element structure — the from_svg root-helper selection is the
        // deferred namespace-aware layer, asserted elsewhere.)
        assert_eq!(
            single_skeleton("<svg>\n<rect></rect>\n<circle></circle>\n</svg>"),
            "<svg><rect></rect><circle></circle></svg>"
        );
    }

    #[test]
    fn svg_text_element_keeps_whitespace() {
        // X6 — the SVG `<text>` exception: whitespace INSIDE an SVG `<text>` is
        // SIGNIFICANT and kept. svelte@5.56.3: `<svg><text>\nhello\n</text></svg>` →
        // `<svg><text>hello</text></svg>` (the leading/trailing edges of the SINGLE
        // text node are trimmed by the standard rule, but it is NOT removed entirely
        // — a `<text>` with interior content keeps it). The discriminating case is a
        // text-only `<text>`: its content survives rather than vanishing.
        assert_eq!(
            single_skeleton("<svg><text>\nhello\n</text></svg>"),
            "<svg><text>hello</text></svg>"
        );
    }

    #[test]
    fn pre_leading_exact_newline_is_discarded() {
        // X9 — a `<pre>` whose FIRST child is exactly `\n` discards it (the browser
        // would, so keeping it breaks hydration). svelte@5.56.3:
        // `<pre>\n<span>x</span></pre>` → `<pre><span>x</span></pre>`.
        assert_eq!(
            single_skeleton("<pre>\n<span>x</span></pre>"),
            "<pre><span>x</span></pre>"
        );
    }

    #[test]
    fn pre_leading_text_with_newline_is_not_discarded() {
        // X9 (negative control) — a `<pre>` whose first child is `\nhello` (NOT
        // exactly `\n`) keeps the newline verbatim (preserve_ws keeps all whitespace,
        // and the first-newline discard only fires for an EXACT `\n` / `\r\n`).
        // svelte@5.56.3: `<pre>\nhello</pre>` → `<pre>\nhello</pre>`.
        assert_eq!(single_skeleton("<pre>\nhello</pre>"), "<pre>\nhello</pre>");
    }

    #[test]
    fn pre_leading_crlf_is_discarded() {
        // X9 — the discard also fires for an exact `\r\n` first child.
        assert_eq!(
            single_skeleton("<pre>\r\n<span>x</span></pre>"),
            "<pre><span>x</span></pre>"
        );
    }

    #[test]
    fn textarea_leading_newline_is_not_discarded() {
        // X9 (negative control) — `<textarea>` does NOT discard a leading newline
        // (only `<pre>` does). svelte@5.56.3: `<textarea>\nhello</textarea>` keeps
        // the `\n`.
        assert_eq!(
            single_skeleton("<textarea>\nhello</textarea>"),
            "<textarea>\nhello</textarea>"
        );
    }
}

// ---------------------------------------------------------------------------
// X7 — `cannot_be_set_statically`: `autofocus` / `muted` / `defaultValue` /
// `defaultChecked` are EXCLUDED from the static `from_html` skeleton and applied
// at runtime via a `$.autofocus(...)` (autofocus) or a DOM property write (the
// others). Modeled as the `NonStaticProperty` runtime op. All EMPIRICALLY pinned
// to svelte@5.56.3.
// ---------------------------------------------------------------------------

mod non_static_property_attrs {
    use super::*;
    use crate::svelte::runtime::ir::{NonStaticPropertyKind, NonStaticPropertyValue, RuntimeOp};

    /// The single serialized `from_html` skeleton for a single-region fixture.
    fn single_skeleton(src: &str) -> String {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        let plan = plan_static_templates(&ir, None);
        let mut htmls: Vec<String> = plan
            .templates
            .iter()
            .filter_map(|t| match t {
                TemplateFactory::FromHtml { html, .. } => Some(html.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            htmls.len(),
            1,
            "expected a single from_html region for {src}"
        );
        htmls.pop().unwrap()
    }

    /// The first `NonStaticProperty` op found in the IR.
    fn first_non_static_property(
        ir: &super::super::ir::SvelteRuntimeIr,
    ) -> (String, NonStaticPropertyKind, NonStaticPropertyValue) {
        ir.ops
            .iter()
            .find_map(|o| match o {
                RuntimeOp::NonStaticProperty { property, .. } => {
                    Some((property.name.clone(), property.kind, property.value.clone()))
                }
                _ => None,
            })
            .expect("a NonStaticProperty op exists")
    }

    #[test]
    fn video_muted_is_excluded_from_skeleton_and_emits_property_write() {
        // X7 — `<video muted>` → `<video></video>` (no `muted` in the skeleton) +
        // a DOM property write `.muted = true`. EMPIRICALLY confirmed against
        // svelte@5.56.3 (`from_html(\`<video></video>\`, 2)` + `video.muted = true`).
        let alloc = Allocator::default();
        let ir = lower("<video muted></video>", &alloc);
        let plan = plan_static_templates(&ir, None);
        let html = match &plan.templates[0] {
            TemplateFactory::FromHtml { html, .. } => html.clone(),
            other => panic!("expected from_html, got {other:?}"),
        };
        assert_eq!(
            html, "<video></video>",
            "muted must be excluded from the skeleton"
        );
        let (name, kind, value) = first_non_static_property(&ir);
        assert_eq!(name, "muted");
        assert_eq!(
            kind,
            NonStaticPropertyKind::DomProperty,
            "muted is a property write"
        );
        assert_eq!(
            value,
            NonStaticPropertyValue::Boolean,
            "valueless muted → boolean true"
        );
    }

    #[test]
    fn input_autofocus_is_excluded_and_emits_autofocus_helper() {
        // X7 — `<input autofocus>` → `<input/>` (no autofocus in the skeleton) +
        // the `$.autofocus(input, true)` helper. EMPIRICALLY confirmed against
        // svelte@5.56.3.
        let alloc = Allocator::default();
        let ir = lower("<input autofocus/>", &alloc);
        assert_eq!(single_skeleton("<input autofocus/>"), "<input/>");
        let (name, kind, value) = first_non_static_property(&ir);
        assert_eq!(name, "autofocus");
        assert_eq!(
            kind,
            NonStaticPropertyKind::Autofocus,
            "autofocus uses $.autofocus"
        );
        assert_eq!(
            value,
            NonStaticPropertyValue::Boolean,
            "valueless autofocus → boolean true"
        );
    }

    #[test]
    fn input_default_value_static_is_excluded_and_emits_property_write() {
        // X7 — `<input defaultValue="x">` → `<input/>` + `.defaultValue = 'x'`.
        // EMPIRICALLY confirmed against svelte@5.56.3.
        let alloc = Allocator::default();
        let ir = lower("<input defaultValue=\"x\"/>", &alloc);
        assert_eq!(single_skeleton("<input defaultValue=\"x\"/>"), "<input/>");
        let (name, kind, value) = first_non_static_property(&ir);
        assert_eq!(name, "defaultValue");
        assert_eq!(kind, NonStaticPropertyKind::DomProperty);
        assert_eq!(
            value,
            NonStaticPropertyValue::Literal("x".to_string()),
            "the static defaultValue carries its literal `x`"
        );
    }

    #[test]
    fn input_default_checked_valueless_is_excluded_and_emits_property_write() {
        // X7 — `<input defaultChecked>` → `<input/>` + `.defaultChecked = true`.
        let alloc = Allocator::default();
        let ir = lower("<input defaultChecked/>", &alloc);
        assert_eq!(single_skeleton("<input defaultChecked/>"), "<input/>");
        let (name, kind, value) = first_non_static_property(&ir);
        assert_eq!(name, "defaultChecked");
        assert_eq!(kind, NonStaticPropertyKind::DomProperty);
        assert_eq!(value, NonStaticPropertyValue::Boolean);
    }

    #[test]
    fn dynamic_default_value_is_excluded_and_carries_the_expression() {
        // X7 — a DYNAMIC `<input defaultValue={x}>` is ALSO excluded from the
        // skeleton (`<input/>`) and carries the expression value
        // (svelte@5.56.3: `.defaultValue = x`).
        let alloc = Allocator::default();
        let src = "<script>let x = $state(\"y\");</script><input defaultValue={x}/>";
        let ir = lower(src, &alloc);
        assert_eq!(single_skeleton(src), "<input/>");
        let (name, kind, value) = first_non_static_property(&ir);
        assert_eq!(name, "defaultValue");
        assert_eq!(kind, NonStaticPropertyKind::DomProperty);
        assert!(
            matches!(value, NonStaticPropertyValue::Expr(_)),
            "a dynamic defaultValue carries an expression value (got {value:?})"
        );
    }

    #[test]
    fn mixed_default_value_retains_the_full_literal_expr_chunk_alternation() {
        // A MIXED `<input defaultValue="a {x} b">` carries the FULL ordered
        // literal/expr alternation (NOT a collapsed lone expression): official
        // emits `input.defaultValue = `a ${x ?? ''} b``, so the IR retains
        // `[Literal("a "), Expr(x), Literal(" b")]`. FAILS against the pre-fix code
        // that dropped the literal chunks to a lone `Expr`.
        use super::super::ir::MixedAttrPart;
        let alloc = Allocator::default();
        let src = "<script>let x = $state(\"y\");</script><input defaultValue=\"a {x} b\"/>";
        let ir = lower(src, &alloc);
        assert_eq!(single_skeleton(src), "<input/>");
        let (name, kind, value) = first_non_static_property(&ir);
        assert_eq!(name, "defaultValue");
        assert_eq!(kind, NonStaticPropertyKind::DomProperty);
        let NonStaticPropertyValue::Mixed(parts) = value else {
            panic!("a mixed defaultValue carries the full chunk list (got {value:?})");
        };
        assert_eq!(parts.len(), 3, "three chunks: literal, expr, literal");
        assert!(
            matches!(&parts[0], MixedAttrPart::Literal(s) if s == "a "),
            "first chunk is the literal `a ` (got {:?})",
            parts[0]
        );
        assert!(
            matches!(&parts[1], MixedAttrPart::Expr(_)),
            "second chunk is the `{{x}}` expression (got {:?})",
            parts[1]
        );
        assert!(
            matches!(&parts[2], MixedAttrPart::Literal(s) if s == " b"),
            "third chunk is the literal ` b` (got {:?})",
            parts[2]
        );
    }

    #[test]
    fn ordinary_static_attribute_is_not_a_non_static_property() {
        // X7 (negative control) — an ordinary `<input value="x">` is a NORMAL
        // static attribute: it STAYS in the skeleton and emits NO NonStaticProperty
        // op. (svelte@5.56.3: `value` is a DOM property but is settable statically —
        // it is NOT in NON_STATIC_PROPERTIES.)
        let alloc = Allocator::default();
        let ir = lower("<input value=\"x\"/>", &alloc);
        assert_eq!(
            single_skeleton("<input value=\"x\"/>"),
            "<input value=\"x\"/>"
        );
        assert!(
            !ir.ops
                .iter()
                .any(|o| matches!(o, RuntimeOp::NonStaticProperty { .. })),
            "an ordinary static `value` attribute is not a non-static property"
        );
    }
}

// ---------------------------------------------------------------------------
// X8 — `is_standalone`: a region whose SOLE cleaned node is a non-dynamic
// `<Component>` (no `--css-var` attr) or a non-dynamic `{@render}` emits NO static
// template — the runtime calls the component / renders the snippet against the
// parent block's anchor directly. EMPIRICALLY pinned to svelte@5.56.3 (the
// standalone `<Foo/>` emits `Foo($$anchor, {})` with no `from_html`).
// ---------------------------------------------------------------------------

mod standalone_component_root {
    use super::*;
    use crate::svelte::runtime::html::StandaloneKind;

    /// All template factories for a fixture.
    fn factories(src: &str) -> Vec<TemplateFactory> {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        plan_static_templates(&ir, None).templates
    }

    /// Whether any factory is a `from_html` whose skeleton contains `needle`.
    fn has_from_html_containing(factories: &[TemplateFactory], needle: &str) -> bool {
        factories
            .iter()
            .any(|f| matches!(f, TemplateFactory::FromHtml { html, .. } if html.contains(needle)))
    }

    #[test]
    fn standalone_component_root_emits_no_template() {
        // X8 — a sole `<Foo/>` root is STANDALONE: NO `from_html` template at all
        // (the runtime calls `Foo($$anchor, {})`). EMPIRICALLY confirmed against
        // svelte@5.56.3. FAILS against the prior `from_html("<!>")` for a component
        // root.
        let src = "<script>let Foo = 1;</script><Foo/>";
        let fs = factories(src);
        assert_eq!(fs.len(), 1, "one root region");
        assert_eq!(
            fs[0],
            TemplateFactory::Standalone {
                kind: StandaloneKind::Component
            },
            "a standalone component root is a Standalone factory (got {:?})",
            fs[0]
        );
        // Negative: NO from_html with a `<!>` anchor for the component.
        assert!(
            !has_from_html_containing(&fs, "<!>"),
            "a standalone component must NOT be a from_html(\"<!>\") template (got {fs:?})"
        );
    }

    #[test]
    fn standalone_component_inside_if_body_emits_no_template() {
        // X8 — `{#if c}<Foo/>{/if}`: the if-BODY region is standalone (NO template);
        // the if-block region itself is a `$.comment()` anchor. EMPIRICALLY
        // confirmed against svelte@5.56.3 (NO from_html, the component mounts via the
        // if anchor). FAILS against the prior `from_html("<!>")` for the if-body.
        let src = "<script>let Foo = 1; let c = true;</script>{#if c}<Foo/>{/if}";
        let fs = factories(src);
        // The if-body standalone region is present, and NO from_html("<!>") exists.
        assert!(
            fs.iter().any(|f| matches!(
                f,
                TemplateFactory::Standalone {
                    kind: StandaloneKind::Component
                }
            )),
            "the if-body has a Standalone component factory (got {fs:?})"
        );
        assert!(
            !has_from_html_containing(&fs, "<!>"),
            "no from_html(\"<!>\") for the standalone if-body component (got {fs:?})"
        );
    }

    #[test]
    fn component_with_css_var_is_not_standalone() {
        // X8 (negative control) — `<Foo --x="red"/>` is NOT standalone (it has a
        // `--css-var` attribute): svelte@5.56.3 emits a `svelte-css-wrapper`
        // `from_html` template. The Verter region is therefore a `from_html`, NOT a
        // Standalone factory.
        let src = "<script>let Foo = 1;</script><Foo --x=\"red\"/>";
        let fs = factories(src);
        assert!(
            !fs.iter()
                .any(|f| matches!(f, TemplateFactory::Standalone { .. })),
            "a component with a --css-var attribute is NOT standalone (got {fs:?})"
        );
        assert!(
            fs.iter()
                .any(|f| matches!(f, TemplateFactory::FromHtml { .. })),
            "a --css-var component is a from_html region (got {fs:?})"
        );
    }

    #[test]
    fn component_with_bind_this_is_still_standalone() {
        // X8 — `<Foo bind:this={r}/>` is STILL standalone (a `bind:this` does NOT
        // break standalone — only a `--css-var` attr / HMR / dynamic does).
        // EMPIRICALLY confirmed against svelte@5.56.3 (`Foo($$anchor, {}), …` with
        // no from_html).
        let src = "<script>let Foo = 1; let r = $state();</script><Foo bind:this={r}/>";
        let fs = factories(src);
        assert_eq!(fs.len(), 1, "one root region (got {fs:?})");
        assert!(
            matches!(
                fs[0],
                TemplateFactory::Standalone {
                    kind: StandaloneKind::Component
                }
            ),
            "a bind:this component root is still standalone (got {:?})",
            fs[0]
        );
    }

    #[test]
    fn two_component_roots_are_not_standalone() {
        // X8 (negative control) — two adjacent component roots are NOT standalone
        // (the region has 2 cleaned nodes): svelte@5.56.3 emits
        // `from_html("<!><!>", 1)`. The Verter region is a multi-root from_html.
        let src = "<script>let Foo=1; let Bar=2;</script><Foo/><Bar/>";
        let fs = factories(src);
        assert!(
            !fs.iter()
                .any(|f| matches!(f, TemplateFactory::Standalone { .. })),
            "two component roots are NOT standalone (got {fs:?})"
        );
        assert!(
            has_from_html_containing(&fs, "<!><!>"),
            "two component roots are a from_html(\"<!><!>\") fragment (got {fs:?})"
        );
    }

    #[test]
    fn component_with_text_sibling_is_not_standalone() {
        // X8 (negative control) — `hi <Foo/>` has a text sibling, so the region is
        // NOT standalone (2 cleaned nodes): svelte@5.56.3 emits `from_html("hi <!>", 1)`.
        let src = "<script>let Foo=1;</script>hi <Foo/>";
        let fs = factories(src);
        assert!(
            !fs.iter()
                .any(|f| matches!(f, TemplateFactory::Standalone { .. })),
            "a component with a text sibling is NOT standalone (got {fs:?})"
        );
    }

    #[test]
    fn local_snippet_render_root_is_standalone() {
        // X8 — a sole `{@render row()}` referencing a LOCAL `{#snippet}` (a
        // non-dynamic, resolved-snippet render) is STANDALONE. The snippet body is a
        // separate region; the render root region emits NO from_html / comment for
        // itself.
        let src = "{#snippet row()}<p>x</p>{/snippet}{@render row()}";
        let fs = factories(src);
        assert!(
            fs.iter().any(|f| matches!(
                f,
                TemplateFactory::Standalone {
                    kind: StandaloneKind::Render
                }
            )),
            "a resolved local-snippet render root is standalone (got {fs:?})"
        );
    }

    #[test]
    fn standalone_component_topology_has_no_from_html_or_append() {
        // X8 — the topology of a standalone `<Foo/>` records NO `FromHtml` helper
        // and NO `$.append` (the component mounts against the anchor directly).
        // EMPIRICALLY confirmed against svelte@5.56.3 (no from_html, no $.append).
        let alloc = Allocator::default();
        let ir = lower("<script>let Foo = 1;</script><Foo/>", &alloc);
        let plan = plan_static_templates(&ir, None);
        let topo = plan_client_topology(&ir, &plan, None);
        assert!(
            !topo.helpers.uses(SvelteHelper::FromHtml),
            "a standalone component records no FromHtml helper"
        );
        assert!(
            !topo.helpers.uses(SvelteHelper::Append),
            "a standalone component records no $.append (no cloned fragment to mount)"
        );
    }
}

// ---------------------------------------------------------------------------
// IR-navigation helpers for the tests above.
// ---------------------------------------------------------------------------

fn find_block<'b>(ir: &'b super::ir::SvelteRuntimeIr) -> &'b BlockIr {
    ir.nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Block(b) => Some(b),
            _ => None,
        })
        .expect("a block node exists")
}

fn find_tag<'b>(ir: &'b super::ir::SvelteRuntimeIr) -> &'b TagIr {
    ir.nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Tag(t) => Some(t),
            _ => None,
        })
        .expect("a tag node exists")
}

fn find_each_block(ir: &super::ir::SvelteRuntimeIr) -> usize {
    ir.nodes
        .iter()
        .position(|n| matches!(n, IrNode::Block(BlockIr::Each { .. })))
        .expect("an each block exists")
}

fn each_block_body(ir: &super::ir::SvelteRuntimeIr, node_idx: usize) -> super::ir::TemplateScopeId {
    match &ir.nodes[node_idx] {
        IrNode::Block(BlockIr::Each { body, .. }) => *body,
        _ => panic!("not an each block"),
    }
}

fn find_snippet_body(ir: &super::ir::SvelteRuntimeIr) -> super::ir::TemplateScopeId {
    ir.nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Block(BlockIr::Snippet { body, .. }) => Some(*body),
            _ => None,
        })
        .expect("a snippet block exists")
}

/// The pending / then / catch body template scopes of the first await block.
#[allow(clippy::type_complexity)]
fn await_bodies(
    ir: &super::ir::SvelteRuntimeIr,
) -> (
    Option<super::ir::TemplateScopeId>,
    Option<super::ir::TemplateScopeId>,
    Option<super::ir::TemplateScopeId>,
) {
    ir.nodes
        .iter()
        .find_map(|n| match n {
            IrNode::Block(BlockIr::Await {
                pending,
                then_body,
                catch_body,
                ..
            }) => Some((*pending, *then_body, *catch_body)),
            _ => None,
        })
        .expect("an await block exists")
}

/// The tag name of the first ELEMENT root of a template scope (for asserting an
/// await branch body's content).
fn scope_root_element_tag(
    ir: &super::ir::SvelteRuntimeIr,
    scope: super::ir::TemplateScopeId,
) -> Option<String> {
    ir.template_scope(scope).roots.iter().find_map(|&id| {
        if let IrNode::Element(el) = ir.node(id) {
            Some(el.tag.clone())
        } else {
            None
        }
    })
}

/// The free references of the event handler in the first element with an Event
/// attribute.
fn handler_references(ir: &super::ir::SvelteRuntimeIr) -> Vec<String> {
    for node in &ir.nodes {
        if let IrNode::Element(el) = node {
            for attr in &el.attrs {
                if let super::ir::AttrIr::Event { handler, .. } = attr {
                    return ir
                        .analysis
                        .expressions
                        .get(*handler)
                        .references
                        .iter()
                        .map(|r| r.name.clone())
                        .collect();
                }
            }
        }
    }
    Vec::new()
}

/// The free references of the first interpolation node.
fn interpolation_references(ir: &super::ir::SvelteRuntimeIr) -> Vec<String> {
    for node in &ir.nodes {
        if let IrNode::Interpolation { expr, .. } = node {
            return ir
                .analysis
                .expressions
                .get(*expr)
                .references
                .iter()
                .map(|r| r.name.clone())
                .collect();
        }
    }
    Vec::new()
}

/// The fragment flag of a template factory.
fn first_fragment_flag(factory: &TemplateFactory) -> Option<TemplateFlag> {
    match factory {
        TemplateFactory::FromHtml { fragment_flag, .. } => *fragment_flag,
        TemplateFactory::TextNode { .. }
        | TemplateFactory::CommentAnchor { .. }
        | TemplateFactory::Standalone { .. } => None,
    }
}

// ===========================================================================
// Official-algorithm conformance: whitespace skeleton (clean_nodes /
// process_children / flush_sequence), entity decoding (decode_character_
// references / validate_code / reg_exp_entity), and event-attribute modeling
// (is_event_attribute / visit_event_attribute / is_capture_event /
// can_delegate_event). Each W1-W7 case below was ground-truthed against the
// pinned svelte@5.56.3 compiler; the asserted output is the official output.
// ===========================================================================

mod official_whitespace_skeleton {
    use super::*;

    /// The `(html, flag)` rows of the planned `from_html` factories, in plan
    /// order (comment-anchor factories excluded).
    fn from_html_rows(src: &str) -> Vec<(String, Option<String>)> {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        plan_static_templates(&ir, None)
            .templates
            .iter()
            .filter_map(|t| match t {
                TemplateFactory::FromHtml {
                    html,
                    fragment_flag,
                    ..
                } => Some((html.clone(), fragment_flag.map(|f| f.literal()))),
                TemplateFactory::TextNode { .. }
                | TemplateFactory::CommentAnchor { .. }
                | TemplateFactory::Standalone { .. } => None,
            })
            .collect()
    }

    /// The single `from_html` skeleton string (panics if not exactly one).
    fn single_html(src: &str) -> String {
        let rows = from_html_rows(src);
        assert_eq!(rows.len(), 1, "expected exactly one from_html for {src:?}");
        rows[0].0.clone()
    }

    #[test]
    fn w1_adjacent_roots_without_whitespace_have_no_separator() {
        // svelte@5.56.3: `<a></a><b></b>` → `<a></a><b></b>` (NO inter-root
        // space) with the multi-root fragment flag. FAILS against the
        // unconditional-separator `synthesize_region` (which yields
        // `<a></a> <b></b>`).
        let rows = from_html_rows("<a></a><b></b>");
        assert_eq!(
            rows,
            vec![("<a></a><b></b>".to_string(), Some("1".to_string()))],
            "adjacent no-whitespace roots are concatenated with no separator"
        );
        // Negative: the buggy synthesized separator must NOT appear.
        assert!(
            !rows[0].0.contains("</a> <b>"),
            "no synthesized space may appear between adjacent roots (got {:?})",
            rows[0].0
        );
    }

    #[test]
    fn w1_adjacent_roots_with_authored_space_preserve_one_space() {
        // svelte@5.56.3: `<a></a> <b></b>` → `<a></a> <b></b>` — the AUTHORED
        // single space is a significant text root and survives. (Discriminates
        // the authored space from a synthesized one: with the unconditional-
        // separator bug BOTH produce one space here, so this is the companion to
        // the no-whitespace case which the bug gets wrong.)
        let rows = from_html_rows("<a></a> <b></b>");
        assert_eq!(
            rows,
            vec![("<a></a> <b></b>".to_string(), Some("1".to_string()))],
            "an authored space between roots is preserved as one space"
        );
    }

    #[test]
    fn w1_three_adjacent_roots_without_whitespace_have_no_separator() {
        // svelte@5.56.3: `<a></a><b></b><c></c>` → `<a></a><b></b><c></c>`.
        assert_eq!(
            single_html("<a></a><b></b><c></c>"),
            "<a></a><b></b><c></c>",
            "three adjacent no-whitespace roots concatenate with no separators"
        );
    }

    #[test]
    fn w2_dynamic_root_after_static_root_is_single_space_no_separator() {
        // svelte@5.56.3: `<a></a>{x}` → `<a></a> ` — the trailing interpolation
        // is a single-space placeholder and there is NO inter-root separator
        // before it. FAILS against the unconditional separator (`<a></a>  `, two
        // spaces).
        let html = single_html("<a></a>{x}");
        assert_eq!(
            html, "<a></a> ",
            "a dynamic root after a static root is exactly one trailing space, no separator"
        );
        // Negative: there must be no DOUBLE space (separator + placeholder).
        assert!(
            !html.contains("  "),
            "no double space (a synthesized separator plus the placeholder) (got {html:?})"
        );
    }

    #[test]
    fn w2_static_dynamic_static_collapses_interp_to_one_space() {
        // svelte@5.56.3: `<a></a>{x}<c></c>` → `<a></a> <c></c>` — the middle
        // interpolation collapses to ONE space between the two static roots, with
        // no extra separators. FAILS against the unconditional separator
        // (`<a></a>   <c></c>`).
        let html = single_html("<a></a>{x}<c></c>");
        assert_eq!(
            html, "<a></a> <c></c>",
            "an interpolation between two static roots collapses to exactly one space"
        );
        assert!(
            !html.contains("  "),
            "no double/triple space from synthesized separators (got {html:?})"
        );
    }

    #[test]
    fn w3_debug_tag_between_text_does_not_split_into_double_space() {
        // svelte@5.56.3: `<div>a {@debug v} b</div>` → `<div>a b</div>` — the
        // {@debug} is a non-rendering node REMOVED by clean_nodes, and the two
        // surrounding text runs `a ` and ` b` merge into a single `a b` (one
        // space). FAILS against a planner that keeps the debug as a run-breaker
        // (yielding `a  b`, two spaces) or drops the surrounding whitespace.
        let html = single_html("<div>a {@debug v} b</div>");
        assert_eq!(
            html, "<div>a b</div>",
            "a {{@debug}} between text is removed and the text runs merge to one space"
        );
        assert!(
            !html.contains("a  b"),
            "the merged text must not carry a double space (got {html:?})"
        );
    }

    #[test]
    fn w3_const_tag_between_text_merges_runs() {
        // svelte@5.56.3: `<div>a {@const v = 1} b</div>` → `<div>a b</div>` — same
        // run-merge behavior for a {@const} (also a non-rendering, hoisted node).
        let html = single_html("<div>a {@const v = 1} b</div>");
        assert_eq!(
            html, "<div>a b</div>",
            "a {{@const}} between text is hoisted and the text runs merge to one space"
        );
    }

    #[test]
    fn leading_whitespace_only_root_run_is_dropped() {
        // svelte@5.56.3: `\n   <div></div>` → `<div></div>` (leading whitespace-
        // only text dropped). Mirrors clean_nodes' leading regular.shift() loop.
        assert_eq!(
            single_html("\n   <div></div>"),
            "<div></div>",
            "a leading whitespace-only root text run is dropped"
        );
    }

    /// The single template factory the planner produced for a region.
    fn single_factory(src: &str) -> TemplateFactory {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        let templates = plan_static_templates(&ir, None).templates;
        assert_eq!(
            templates.len(),
            1,
            "expected exactly one template factory for {src:?}"
        );
        templates[0].clone()
    }

    #[test]
    fn pure_text_only_region_is_a_seeded_text_node_not_from_html() {
        // svelte@5.56.3: a region that is a single PURE-text run (`hello`) is
        // created as a SEEDED text node — `$.text('hello')` — NOT a `from_html`
        // clone and NOT a comment anchor. This is a RUNTIME distinction (a text
        // node vs a cloned template). FAILS against a `from_html("hello")` plan.
        let factory = single_factory("hello world");
        assert_eq!(
            factory,
            TemplateFactory::TextNode {
                seed: Some("hello world".to_string())
            },
            "a pure-text root region is a `$.text('hello world')` seeded text node"
        );
    }

    #[test]
    fn interpolation_only_region_is_an_unseeded_text_node() {
        // svelte@5.56.3: a region that is a single interpolation (`{a}`) — or
        // interpolation-only run (`{a}{b}`) — is an UNSEEDED text node `$.text()`
        // (the reactive `$.set_text` fills it), NOT a `from_html` clone and NOT a
        // comment anchor. FAILS against the prior `from_html(" ")` plan (which would
        // clone a literal-space text node) and against a `CommentAnchor` plan.
        let factory = single_factory("<script>let a = $state(1);</script>{a}");
        assert_eq!(
            factory,
            TemplateFactory::TextNode { seed: None },
            "an interpolation-only root region is an unseeded `$.text()` node"
        );
        // Negatives: it must NOT be a from_html clone (the old `from_html(\" \")`
        // bug) nor a comment anchor (the older block-only-root misclassification).
        assert!(
            !matches!(factory, TemplateFactory::FromHtml { .. }),
            "an interpolation-only region must not clone a from_html template"
        );
        assert!(
            !matches!(factory, TemplateFactory::CommentAnchor { .. }),
            "an interpolation-only region must not be a comment anchor"
        );

        // `{a}{b}` (two interpolations, one merged text node) is also unseeded.
        let two = single_factory("<script>let a = $state(1), b = $state(2);</script>{a}{b}");
        assert_eq!(
            two,
            TemplateFactory::TextNode { seed: None },
            "an interpolation-only `{{a}}{{b}}` region is one unseeded `$.text()` node"
        );
    }

    #[test]
    fn text_plus_interpolation_region_is_an_unseeded_text_node() {
        // svelte@5.56.3: `hi {a}!` is one text run with an interpolation → an
        // UNSEEDED `$.text()` (the reactive system fills the whole node), NOT a
        // from_html clone of the static parts.
        let factory = single_factory("<script>let a = $state(1);</script>hi {a}!");
        assert_eq!(
            factory,
            TemplateFactory::TextNode { seed: None },
            "a text+interpolation root region is an unseeded `$.text()` node"
        );
    }

    #[test]
    fn interpolation_with_a_sibling_element_is_from_html_not_a_text_node() {
        // svelte@5.56.3: once a region has an element/block sibling
        // (`{a}<div></div>`) it is NO LONGER text-first — it is a `from_html`
        // region (` <div></div>`, fragment-flagged). The text-node factory applies
        // ONLY when the WHOLE region is a single text run.
        let factory = single_factory("<script>let a = $state(1);</script>{a}<div></div>");
        match factory {
            TemplateFactory::FromHtml {
                html,
                fragment_flag,
                ..
            } => {
                assert_eq!(html, " <div></div>");
                assert!(
                    fragment_flag.is_some(),
                    "the 2-root region is fragment-flagged"
                );
            }
            other => panic!("expected a from_html region, got {other:?}"),
        }
    }

    #[test]
    fn html_comment_is_dropped_and_does_not_break_a_text_run() {
        // svelte@5.56.3 (default `preserveComments: false`): an HTML comment is
        // DROPPED from the skeleton — it occupies no DOM position and does NOT break
        // a surrounding text run. `<div>before<!-- c -->after</div>` →
        // `<div>beforeafter</div>` (the two text runs merge). FAILS against a planner
        // that emits a `<!>` anchor for the comment (`before<!>after`).
        assert_eq!(
            single_html("<div>before<!-- c -->after</div>"),
            "<div>beforeafter</div>",
            "an HTML comment is dropped and the surrounding text runs merge"
        );
        assert!(
            !single_html("<div>before<!-- c -->after</div>").contains("<!>"),
            "a dropped comment must not leave a `<!>` anchor"
        );
    }

    #[test]
    fn html_comment_between_elements_is_dropped_no_anchor() {
        // svelte@5.56.3: `<a></a><!-- c --><b></b>` → `<a></a><b></b>` — the comment
        // is dropped with NO `<!>` and NO separator. FAILS against a planner that
        // serializes the comment as a `<!>` (`<a></a><!><b></b>`).
        assert_eq!(
            single_html("<a></a><!-- c --><b></b>"),
            "<a></a><b></b>",
            "a comment between elements is dropped (no anchor, no separator)"
        );
    }

    #[test]
    fn valueless_boolean_attribute_serializes_with_empty_quoted_value() {
        // svelte@5.56.3: a valueless boolean attribute is emitted as `name=""` in the
        // cloned skeleton, NOT bare `name`. `<input disabled>` →
        // `<input disabled=""/>`. FAILS against a planner that emits bare ` disabled`.
        assert_eq!(
            single_html("<input disabled>"),
            "<input disabled=\"\"/>",
            "a valueless boolean attribute serializes as `disabled=\"\"`"
        );
        // Multiple valueless attrs each get `=""`.
        assert_eq!(
            single_html("<button disabled hidden>x</button>"),
            "<button disabled=\"\" hidden=\"\">x</button>",
            "each valueless attribute serializes with an empty quoted value"
        );
    }
}

mod official_entity_decode_attribute {
    use super::*;

    fn attr_value(src: &str) -> String {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        let html = match &plan_static_templates(&ir, None).templates[0] {
            TemplateFactory::FromHtml { html, .. } => html.clone(),
            other => panic!("expected a from_html factory, got {other:?}"),
        };
        let start = html.find("title=\"").expect("title attr") + "title=\"".len();
        let rest = &html[start..];
        let end = rest.find('"').expect("closing quote");
        rest[..end].to_string()
    }

    #[test]
    fn w4_semicolonless_decimal_numeric_entity_decodes() {
        // svelte@5.56.3: `title="&#65"` (NO trailing `;`) → `A`. The official
        // numeric pattern `#(?:x[a-fA-F\d]+|\d+)(?:;)?` makes the `;` OPTIONAL.
        // FAILS against the current numeric decoder which requires a `;`.
        assert_eq!(
            attr_value("<a title=\"&#65\">x</a>"),
            "A",
            "a semicolonless decimal numeric entity `&#65` decodes to A"
        );
    }

    #[test]
    fn w4_semicolonless_hex_numeric_entity_decodes() {
        // svelte@5.56.3: `title="&#x41"` (NO `;`) → `A`.
        assert_eq!(
            attr_value("<a title=\"&#x41\">x</a>"),
            "A",
            "a semicolonless hex numeric entity `&#x41` decodes to A"
        );
    }

    #[test]
    fn w4_semicolonless_numeric_entity_then_text_decodes_and_keeps_text() {
        // svelte@5.56.3: `title="&#65B"` → `AB` — the numeric run ends at the
        // first non-digit and the trailing text is preserved.
        assert_eq!(
            attr_value("<a title=\"&#65B\">x</a>"),
            "AB",
            "a semicolonless numeric entity stops at the first non-digit, text kept"
        );
    }

    #[test]
    fn w7_legacy_named_entity_blocked_by_following_underscore() {
        // svelte@5.56.3: `title="&copy_x"` → `&amp;copy_x` — the legacy no-`;`
        // boundary `\b(?!=)` treats `_` as a WORD char, so the `&copy` reference
        // does NOT match (a following word char blocks it). FAILS against the
        // current `is_ascii_alphanumeric()` boundary which EXCLUDES `_` (and so
        // would wrongly decode `©_x`).
        assert_eq!(
            attr_value("<a title=\"&copy_x\">x</a>"),
            "&amp;copy_x",
            "a following `_` blocks the legacy no-semicolon `&copy` (underscore is a word char)"
        );
        // Negative: the wrongly-decoded `©` must NOT appear.
        assert!(
            !attr_value("<a title=\"&copy_x\">x</a>").contains('\u{00a9}'),
            "the legacy entity must not decode when followed by `_`"
        );
    }

    #[test]
    fn w7_legacy_named_entity_followed_by_space_still_decodes() {
        // svelte@5.56.3: `title="&copy x"` → `© x` — a following SPACE is a word
        // boundary and not `=`, so the legacy `&copy` decodes. (Companion to the
        // `_`-blocked case: confirms the boundary is word-char based, not "any
        // non-`;`".)
        assert_eq!(
            attr_value("<a title=\"&copy x\">x</a>"),
            "\u{00a9} x",
            "a following space is a word boundary, so legacy `&copy` decodes to ©"
        );
    }
}

mod official_entity_decode_text {
    use super::*;
    use crate::svelte::runtime::entity_decode::decode_text_entities_for_test as dt;
    use crate::svelte::runtime::html::TemplateFactory;

    /// The `$.text(seed)` seed of a single-root text-first region (`$state` makes it
    /// reactive-free static text), or `None` for a non-text-first region.
    fn text_seed(src: &str) -> Option<String> {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        plan_static_templates(&ir, None)
            .templates
            .into_iter()
            .find_map(|t| match t {
                TemplateFactory::TextNode { seed } => Some(seed),
                _ => None,
            })
            .flatten()
    }

    #[test]
    fn text_named_entity_seed_decodes() {
        // A text-first root `&copy;` seeds `$.text('©')` — the text-context decode
        // (official `decode_character_references(text, false)`).
        assert_eq!(dt("&copy;"), "\u{00a9}");
        assert_eq!(text_seed("&copy;").as_deref(), Some("\u{00a9}"));
    }

    #[test]
    fn text_numeric_and_hex_entity_seed_decodes() {
        assert_eq!(dt("&#65;"), "A");
        assert_eq!(dt("&#x41;"), "A");
        assert_eq!(text_seed("&#65;").as_deref(), Some("A"));
    }

    #[test]
    fn text_uppercase_x_prefix_is_not_a_numeric_reference() {
        // The official pattern `#(?:x[a-fA-F\d]+|\d+)(?:;)?` accepts a
        // LOWERCASE `x` prefix only — `&#X41;` is NOT a character reference
        // and stays literal in BOTH decode contexts. Hex DIGITS keep both
        // cases (`&#x4A1;` → U+04A1).
        assert_eq!(dt("&#X41;"), "&#X41;");
        assert_eq!(dt("&#x4A1;"), "\u{04A1}");
        // Control: the lowercase prefix decodes (the two-sided discriminator).
        assert_eq!(dt("&#x41;"), "A");
    }

    #[test]
    fn text_mixed_entity_seed_decodes_each_reference() {
        // `a &copy; b` → `a © b` (decode each reference, keep the literal text).
        assert_eq!(dt("a &copy; b"), "a \u{00a9} b");
        assert_eq!(text_seed("a &copy; b").as_deref(), Some("a \u{00a9} b"));
    }

    #[test]
    fn text_numeric_overflow_decodes_to_nul() {
        // `&#9999999999;` overflows; official `parseInt` → out-of-range →
        // `validate_code` → NUL. A `u32`-overflowing value also maps to NUL.
        assert_eq!(dt("&#9999999999;"), "\u{0}");
        // The 12-digit `&#999999999999;` likewise (the brief's explicit case).
        assert_eq!(dt("&#999999999999;"), "\u{0}");
    }

    #[test]
    fn text_context_legacy_no_semicolon_decodes_unconditionally() {
        // The TEXT-content entity pattern has NO `\b(?!=)` boundary, so a legacy
        // no-`;` named reference decodes UNCONDITIONALLY — `&copy=x` → `©=x` in TEXT
        // context (vs the ATTRIBUTE context where the following `=` BLOCKS it). This
        // is the content-vs-attribute difference between the two decode paths.
        assert_eq!(dt("&copy=x"), "\u{00a9}=x");
        // The attribute path keeps it literal (blocked by `=`).
        use crate::svelte::runtime::entity_decode::decode_attribute_entities_for_test as da;
        assert_eq!(da("&copy=x"), "&copy=x");
    }

    #[test]
    fn text_unknown_entity_is_kept_literal() {
        // An unknown `&bogus;` is NOT decoded — its `&` is kept literal (the text
        // seed is a JS string, NOT re-escaped, so it stays `&bogus;`).
        assert_eq!(dt("&bogus;"), "&bogus;");
    }
}

mod official_event_attribute_modeling {
    use super::*;
    use crate::svelte::runtime::ir::{AttrIr, IrNode};

    /// The first `AttrIr::Event` found anywhere in the IR's element/component
    /// attribute lists, as `(event_type, delegated, capture, modifiers)`.
    fn first_event(ir: &super::super::ir::SvelteRuntimeIr) -> (String, bool, bool, Vec<String>) {
        for node in &ir.nodes {
            let attrs = match node {
                IrNode::Element(el) => &el.attrs,
                IrNode::Component(c) => &c.attrs,
                _ => continue,
            };
            for attr in attrs {
                if let AttrIr::Event {
                    event_type,
                    delegated,
                    capture,
                    modifiers,
                    ..
                } = attr
                {
                    return (event_type.clone(), *delegated, *capture, modifiers.clone());
                }
            }
        }
        panic!("no AttrIr::Event found");
    }

    /// Whether the IR has ANY dynamic (non-event) attribute named `name`.
    fn has_dynamic_attr(ir: &super::super::ir::SvelteRuntimeIr, name: &str) -> bool {
        ir.nodes.iter().any(|node| {
            let attrs = match node {
                IrNode::Element(el) => &el.attrs,
                IrNode::Component(c) => &c.attrs,
                _ => return false,
            };
            attrs.iter().any(|a| {
                matches!(a, AttrIr::Dynamic { name: n, .. } if n == name)
                    || matches!(a, AttrIr::Mixed { name: n, .. } if n == name)
            })
        })
    }

    #[test]
    fn w5_onclickcapture_normalizes_to_click_with_capture_not_delegated() {
        // svelte@5.56.3: `onclickcapture={h}` → `$.event('click', …, true)` — the
        // event NAME is `click`, capture is true, and it is NOT delegated (uses
        // `$.event`, no `$.delegate([...])` set). FAILS against the current
        // strip-`on`-only lowering (event_type `clickcapture`, no capture, and
        // since `clickcapture` is not in the delegated set it would be a direct
        // listener with the WRONG name).
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let h = () => {};</script><button onclickcapture={h}>x</button>",
            &alloc,
        );
        let (event_type, delegated, capture, _mods) = first_event(&ir);
        assert_eq!(
            event_type, "click",
            "the capture suffix is stripped → `click`"
        );
        assert!(capture, "the capture flag is set");
        assert!(
            !delegated,
            "a capture event is NOT delegated (the raw `clickcapture` is not in the delegated set)"
        );
    }

    #[test]
    fn w5_plain_onclick_is_click_delegated_not_capture() {
        // svelte@5.56.3: `onclick={h}` → `$.delegated('click', …)` + `$.delegate`
        // — name `click`, delegated, NOT capture.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let h = () => {};</script><button onclick={h}>x</button>",
            &alloc,
        );
        let (event_type, delegated, capture, _mods) = first_event(&ir);
        assert_eq!(event_type, "click");
        assert!(
            delegated,
            "a plain onclick on a RegularElement is delegated"
        );
        assert!(!capture, "a plain onclick is not a capture handler");
    }

    #[test]
    fn w5_ongotpointercapture_is_not_a_capture_event() {
        // svelte@5.56.3: `ongotpointercapture={h}` → `$.event('gotpointercapture',
        // …)` — NOT capture (the name ends in `capture` but `is_capture_event`
        // EXCLUDES `gotpointercapture`/`lostpointercapture`). The event name is
        // kept WHOLE. FAILS against a naive `ends_with("capture")` strip (which
        // would wrongly yield name `gotpointer` + capture true).
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let h = () => {};</script><div ongotpointercapture={h}></div>",
            &alloc,
        );
        let (event_type, delegated, capture, _mods) = first_event(&ir);
        assert_eq!(
            event_type, "gotpointercapture",
            "gotpointercapture is NOT a capture event — the name is kept whole"
        );
        assert!(!capture, "gotpointercapture is not a capture-phase handler");
        assert!(!delegated, "gotpointercapture is not in the delegated set");
    }

    #[test]
    fn w5_ongotpointercapturecapture_strips_to_gotpointercapture_with_capture() {
        // svelte@5.56.3: `ongotpointercapturecapture={h}` →
        // `$.event('gotpointercapture', …, true)` — the OUTER `capture` is
        // stripped (leaving `gotpointercapture`), capture true. This is the tricky
        // double-suffix case `is_capture_event` handles via the exact exclusion.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let h = () => {};</script><div ongotpointercapturecapture={h}></div>",
            &alloc,
        );
        let (event_type, _delegated, capture, _mods) = first_event(&ir);
        assert_eq!(
            event_type, "gotpointercapture",
            "the outer capture suffix is stripped, leaving gotpointercapture"
        );
        assert!(capture, "the doubled-capture form IS a capture handler");
    }

    #[test]
    fn w6_quoted_single_expression_onclick_is_an_event_not_a_dynamic_attr() {
        // svelte@5.56.3: `onclick="{() => x()}"` (a QUOTED value with exactly one
        // expression chunk and no literal text) is treated as an EVENT —
        // `$.delegated('click', …)` + `$.delegate(['click'])`. FAILS against the
        // current lowering which only matches the bare `{expr}` value form and
        // routes the quoted Mixed form to a dynamic attribute.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let x = () => {};</script><button onclick=\"{() => x()}\">y</button>",
            &alloc,
        );
        let (event_type, delegated, capture, _mods) = first_event(&ir);
        assert_eq!(
            event_type, "click",
            "the quoted single-expr onclick is event `click`"
        );
        assert!(delegated, "the quoted onclick is a delegated event");
        assert!(!capture, "the quoted onclick is not capture");
        // Negative: it must NOT have been lowered to a dynamic `onclick` attr.
        assert!(
            !has_dynamic_attr(&ir, "onclick"),
            "the quoted single-expr onclick must not be a dynamic attribute"
        );
    }

    /// Whether a lowering result is the `attribute_invalid_event_handler`
    /// diagnostic (the official compile error for an `on*` attribute whose value is
    /// not a single expression).
    fn is_invalid_event_handler_error(
        result: &Result<super::super::ir::SvelteRuntimeIr, super::super::RuntimeLoweringErrors>,
    ) -> bool {
        matches!(result, Err(errors)
            if errors.diagnostics.iter().any(|d| d.code == "svelte-runtime-invalid-event-handler"))
    }

    #[test]
    fn onclick_with_surrounding_whitespace_is_invalid_event_handler_error() {
        // X3 — `onclick=" {h} "` is a >1-chunk value (`[Text(" "), ExpressionTag,
        // Text(" ")]`), so `is_expression_attribute` is false. Because the name
        // starts with `on` and is longer than `on`, svelte@5.56.3 raises
        // `attribute_invalid_event_handler` (EMPIRICALLY confirmed against the
        // pinned compiler). It is NOT lowered as a normal attribute. FAILS against
        // the prior behavior that fell through to a dynamic/mixed attribute.
        let alloc = Allocator::default();
        let result = lower_result(
            "<script>let h = () => {};</script><button onclick=\" {h} \">y</button>",
            &alloc,
        );
        assert!(
            is_invalid_event_handler_error(&result),
            "a whitespace-surrounded onclick value is the attribute_invalid_event_handler error"
        );
    }

    #[test]
    fn onclick_with_text_and_expression_is_invalid_event_handler_error() {
        // X3 — `onclick="x{h}"` is a >1-chunk value (`[Text("x"), ExpressionTag]`)
        // → `attribute_invalid_event_handler` (confirmed against svelte@5.56.3).
        let alloc = Allocator::default();
        let result = lower_result(
            "<script>let h = () => {};</script><button onclick=\"x{h}\">y</button>",
            &alloc,
        );
        assert!(
            is_invalid_event_handler_error(&result),
            "a text-and-expression onclick value is the attribute_invalid_event_handler error"
        );
    }

    #[test]
    fn onclick_with_two_expressions_is_invalid_event_handler_error() {
        // X3 — `onclick="{h}{h}"` is a >1-chunk value (two ExpressionTags) →
        // `attribute_invalid_event_handler` (confirmed against svelte@5.56.3).
        let alloc = Allocator::default();
        let result = lower_result(
            "<script>let h = () => {};</script><button onclick=\"{h}{h}\">y</button>",
            &alloc,
        );
        assert!(
            is_invalid_event_handler_error(&result),
            "a two-expression onclick value is the attribute_invalid_event_handler error"
        );
    }

    #[test]
    fn onclick_with_plain_text_value_is_invalid_event_handler_error() {
        // X3 — `onclick="text"` is a single Text chunk (not an ExpressionTag), so
        // `is_expression_attribute` is false → `attribute_invalid_event_handler`
        // (confirmed against svelte@5.56.3).
        let alloc = Allocator::default();
        let result = lower_result("<button onclick=\"text\">y</button>", &alloc);
        assert!(
            is_invalid_event_handler_error(&result),
            "a plain-text onclick value is the attribute_invalid_event_handler error"
        );
    }

    #[test]
    fn valueless_onclick_is_invalid_event_handler_error() {
        // X3 — a valueless `onclick` (`value === true`) is not an expression
        // attribute, and the name is longer than `on` →
        // `attribute_invalid_event_handler` (confirmed against svelte@5.56.3).
        let alloc = Allocator::default();
        let result = lower_result("<button onclick>y</button>", &alloc);
        assert!(
            is_invalid_event_handler_error(&result),
            "a valueless onclick is the attribute_invalid_event_handler error"
        );
    }

    #[test]
    fn bare_on_attribute_is_not_an_error_and_not_an_event() {
        // X1/X3 boundary — a bare `on` attribute has name length EXACTLY 2, so the
        // `name.length > 2` error gate does NOT fire; `on="text"` is a normal static
        // attribute and `on` valueless is a valueless static attribute (confirmed
        // against svelte@5.56.3: `<button on="text">` → `from_html`, no event/error).
        let alloc = Allocator::default();
        let ir = lower("<button on=\"text\">y</button>", &alloc);
        let has_event = ir.nodes.iter().any(|node| {
            matches!(node, IrNode::Element(el)
                if el.attrs.iter().any(|a| matches!(a, AttrIr::Event { .. })))
        });
        assert!(!has_event, "a bare `on` attribute is not an event");
        // It is a static attribute (no diagnostic).
        let result = lower_result("<button on=\"text\">y</button>", &alloc);
        assert!(result.is_ok(), "a bare `on` attribute is not an error");
    }

    #[test]
    fn event_name_gating_follows_is_event_attribute_not_lowercase() {
        // X1 — the official `is_event_attribute` rule is `is_expression_attribute &&
        // name.startsWith('on')`, with NO lowercase-only filter and NO non-empty
        // filter. The event name is `name.slice(2)` (capture-normalized). Each row
        // `(attribute_name, expected_event_type)` is EMPIRICALLY confirmed against
        // svelte@5.56.3 (`$.event('<type>', node, h)`). This FAILS against the prior
        // `name.chars().all(is_ascii_lowercase) && !raw.is_empty()` gate that left
        // `onClick`/`onfoo-bar`/`on`/`on1`/`on_click` as non-events.
        let alloc = Allocator::default();
        let cases: &[(&str, &str)] = &[
            ("onClick", "Click"),     // mixed-case: NOT lowered-filtered
            ("onfoo1", "foo1"),       // trailing digit
            ("onfoo-bar", "foo-bar"), // hyphen in name
            ("onfoo_bar", "foo_bar"), // underscore in name
            ("on", ""),               // bare `on`: event name is the empty string
            ("on1", "1"),             // name is `1`
            ("on_click", "_click"),   // name is `_click`
            ("onclick", "click"),     // baseline lowercase
        ];
        for (attr_name, expected_type) in cases {
            let src =
                format!("<script>let h = () => {{}};</script><button {attr_name}={{h}}>y</button>");
            let ir = lower(&src, &alloc);
            let (event_type, _delegated, _capture, _mods) = first_event(&ir);
            assert_eq!(
                &event_type, expected_type,
                "`{attr_name}={{h}}` is an event with type `{expected_type}` (name.slice(2))"
            );
        }
    }

    #[test]
    fn non_delegated_on_events_use_direct_listener_not_delegation() {
        // X1 — none of the non-DELEGATED_EVENTS names (`Click` capitalized, `foo1`,
        // `foo-bar`, `''`, `1`, `_click`) are in the delegated set, so they are
        // direct `$.event` listeners, never `$.delegated` (confirmed against
        // svelte@5.56.3). Only the canonical lowercase `click` delegates.
        let alloc = Allocator::default();
        for attr_name in ["onClick", "onfoo1", "onfoo-bar", "on", "on1", "on_click"] {
            let src =
                format!("<script>let h = () => {{}};</script><button {attr_name}={{h}}>y</button>");
            let ir = lower(&src, &alloc);
            let (_event_type, delegated, _capture, _mods) = first_event(&ir);
            assert!(
                !delegated,
                "`{attr_name}` is not in DELEGATED_EVENTS, so it is a direct listener"
            );
        }
    }

    #[test]
    fn modern_onclick_delegates_but_legacy_on_click_does_not() {
        // X2 — a MODERN `onclick={h}` on a regular element is DELEGATED
        // (`$.delegated('click', …)` + `$.delegate(['click'])`); a LEGACY
        // `on:click={h}` directive is NEVER delegated (`$.event('click', …)`) — the
        // official `OnDirective.js` always passes `delegated=false`. Both are
        // EMPIRICALLY confirmed against svelte@5.56.3. This FAILS against the prior
        // `!capture && can_delegate_event(local)` that delegated legacy `on:click`.
        let alloc = Allocator::default();
        let modern = lower(
            "<script>let h = () => {};</script><button onclick={h}>y</button>",
            &alloc,
        );
        let (m_type, m_delegated, _c, _m) = first_event(&modern);
        assert_eq!(m_type, "click");
        assert!(m_delegated, "modern onclick={{h}} delegates");

        let legacy = lower(
            "<script>let h = () => {};</script><button on:click={h}>y</button>",
            &alloc,
        );
        let (l_type, l_delegated, l_capture, _m) = first_event(&legacy);
        assert_eq!(l_type, "click", "the legacy directive name stays `click`");
        assert!(
            !l_delegated,
            "a legacy on:click directive is NEVER delegated (build_event(.., false))"
        );
        assert!(!l_capture, "no capture modifier here");
    }

    #[test]
    fn legacy_on_input_directive_is_not_delegated() {
        // X2 — `on:input={h}` (input IS in DELEGATED_EVENTS) is STILL a direct
        // `$.event('input', …)` because it is a legacy directive (confirmed against
        // svelte@5.56.3). The delegation decision is form-based, not name-based.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let h = () => {};</script><input on:input={h}/>",
            &alloc,
        );
        let (event_type, delegated, _c, _m) = first_event(&ir);
        assert_eq!(event_type, "input");
        assert!(
            !delegated,
            "a legacy on:input directive is not delegated despite `input` being delegatable"
        );
    }

    #[test]
    fn legacy_on_click_capture_modifier_is_capture_not_delegated() {
        // svelte@5.56.3: `on:click|capture={h}` → `$.event('click', …, true)` — the
        // legacy `|capture` modifier sets capture true; a capture handler is NOT
        // delegated. The event NAME stays `click` (the modifier is not part of the
        // name).
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let h = () => {};</script><button on:click|capture={h}>x</button>",
            &alloc,
        );
        let (event_type, delegated, capture, modifiers) = first_event(&ir);
        assert_eq!(
            event_type, "click",
            "the legacy directive name stays `click`"
        );
        assert!(capture, "the `|capture` modifier sets the capture flag");
        assert!(
            !delegated,
            "a capture handler is not delegated, even in legacy directive form"
        );
        assert!(
            modifiers.iter().any(|m| m == "capture"),
            "the `capture` modifier is recorded"
        );
    }

    #[test]
    fn capture_event_is_excluded_from_delegated_topology_set() {
        // The delegated-event topology SET must EXCLUDE a capture handler: a
        // `onclickcapture` must not put `click` into `$.delegate([...])`.
        // svelte@5.56.3 emits no `$.delegate` for a pure-capture component.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let h = () => {};</script><button onclickcapture={h}>x</button>",
            &alloc,
        );
        let plan = plan_static_templates(&ir, None);
        let topo = plan_client_topology(&ir, &plan, None);
        assert!(
            !topo.delegated_events.contains("click"),
            "a capture handler must not enter the delegated set"
        );
        assert!(
            !topo.helpers.uses(SvelteHelper::Delegate),
            "no `$.delegate([...])` set is declared for a pure-capture component"
        );
    }

    // -- Y1: host-aware event lowering (the official `metadata.delegated`
    //    parent-kind rule) -------------------------------------------------------

    #[test]
    fn component_event_is_a_forwarded_prop_under_the_original_name_not_an_event() {
        // svelte@5.56.3: `<Foo onclick={h}/>` forwards the handler as a PLAIN PROP
        // keyed by the ORIGINAL attribute name `onclick` (`Foo($$anchor, { onclick:
        // h })`) — it is NOT a DOM event and NEVER delegated. The IR carries it as an
        // `AttrIr::Dynamic { name: "onclick" }`, NOT an `AttrIr::Event`. FAILS against
        // the pre-fix code that flagged a component-hosted `onclick` delegated.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>import Foo from './Foo.svelte'; let h = () => {};</script><Foo onclick={h} />",
            &alloc,
        );
        // No `AttrIr::Event` anywhere (the handler is a prop, not an event).
        let has_event = ir.nodes.iter().any(|n| {
            let attrs = match n {
                IrNode::Component(c) => &c.attrs,
                IrNode::Element(el) => &el.attrs,
                _ => return false,
            };
            attrs.iter().any(|a| matches!(a, AttrIr::Event { .. }))
        });
        assert!(!has_event, "a component `onclick` is NOT an AttrIr::Event");
        // It IS a dynamic prop under the ORIGINAL name `onclick` (not `click`).
        assert!(
            has_dynamic_attr(&ir, "onclick"),
            "a component `onclick` is a forwarded prop under the original name `onclick`"
        );
        assert!(
            !has_dynamic_attr(&ir, "click"),
            "the forwarded prop keeps the `on` prefix (it is `onclick`, not `click`)"
        );
        // It is NOT delegated.
        let plan = plan_static_templates(&ir, None);
        let topo = plan_client_topology(&ir, &plan, None);
        assert!(
            !topo.delegated_events.contains("click"),
            "a component-hosted onclick never enters the delegated set"
        );
    }

    #[test]
    fn svelte_element_event_is_a_dynamic_attribute_not_a_dom_event() {
        // svelte@5.56.3: `<svelte:element this={tag} onclick={h}>` runs the handler
        // through `$.attribute_effect` (a runtime spread), NOT a DOM `$.event`/
        // `$.delegated`. The IR carries the `onclick` as an `AttrIr::Dynamic`
        // attribute (NOT an `AttrIr::Event`), and it is never delegated.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let tag=$state('span'); let h = () => {};</script><svelte:element this={tag} onclick={h}>x</svelte:element>",
            &alloc,
        );
        let has_event = ir
            .nodes
            .iter()
            .any(|n| matches!(n, IrNode::Special(s) if s.attrs.iter().any(|a| matches!(a, AttrIr::Event { .. }))));
        assert!(
            !has_event,
            "a <svelte:element> `onclick` is NOT a DOM AttrIr::Event"
        );
        let has_onclick_attr = ir.nodes.iter().any(|n| matches!(n, IrNode::Special(s) if s.attrs.iter().any(|a| matches!(a, AttrIr::Dynamic { name, .. } if name == "onclick"))));
        assert!(
            has_onclick_attr,
            "a <svelte:element> `onclick` is a dynamic attribute (the attribute_effect surface)"
        );
        let plan = plan_static_templates(&ir, None);
        let topo = plan_client_topology(&ir, &plan, None);
        assert!(
            !topo.delegated_events.contains("click"),
            "a <svelte:element> onclick never enters the delegated set"
        );
    }

    #[test]
    fn window_body_document_events_are_direct_globals_never_delegated() {
        // svelte@5.56.3: a window/body/document `onclick` is a DIRECT global
        // `$.event('click', $.window|$.document.body|$.document, h)` — NEVER delegated
        // (the official `metadata.delegated` requires a `RegularElement` parent). The
        // IR carries an `AttrIr::Event` with `delegated = false` for each.
        let alloc = Allocator::default();
        for (host, _global) in [
            ("svelte:window", "window"),
            ("svelte:body", "body"),
            ("svelte:document", "document"),
        ] {
            let src = format!("<script>let h = () => {{}};</script><{host} onclick={{h}} />");
            let ir = lower(&src, &alloc);
            // The event is an `AttrIr::Event` on the SPECIAL node (window/body/
            // document) — find it there (the shared `first_event` walks only
            // element/component nodes).
            let (event_type, delegated) = ir
                .nodes
                .iter()
                .find_map(|n| match n {
                    IrNode::Special(s) => s.attrs.iter().find_map(|a| match a {
                        AttrIr::Event {
                            event_type,
                            delegated,
                            ..
                        } => Some((event_type.clone(), *delegated)),
                        _ => None,
                    }),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no AttrIr::Event on the {host} special node"));
            assert_eq!(event_type, "click", "{host} onclick → event type click");
            assert!(
                !delegated,
                "a {host} global listener is a DIRECT $.event, never delegated"
            );
            let plan = plan_static_templates(&ir, None);
            let topo = plan_client_topology(&ir, &plan, None);
            assert!(
                !topo.delegated_events.contains("click"),
                "{host} onclick never enters the delegated set"
            );
        }
    }

    #[test]
    fn regular_element_delegated_click_still_delegates() {
        // Control: the host-aware rule does NOT regress a regular intrinsic element —
        // `<button onclick={h}>` is still a delegated DOM event (`can_delegate_event`
        // + a `RegularElement` parent), entering the delegated set.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>let h = () => {};</script><button onclick={h}>x</button>",
            &alloc,
        );
        let (event_type, delegated, _capture, _mods) = first_event(&ir);
        assert_eq!(event_type, "click");
        assert!(delegated, "a regular element onclick is delegated");
        let plan = plan_static_templates(&ir, None);
        let topo = plan_client_topology(&ir, &plan, None);
        assert!(
            topo.delegated_events.contains("click"),
            "a regular element delegated click enters the delegated set"
        );
    }
}

// ===========================================================================
// Table-driven UNIT tests against official-derived cases — pinned independently
// of the corpus, so the algorithm stays correct even if no fixture exercises a
// given case. These call the runtime's PUBLIC decode/event helpers directly.
// ===========================================================================

mod official_entity_decode_unit_table {
    use crate::svelte::runtime::entity_decode::decode_attribute_entities_for_test;

    #[test]
    fn decode_attribute_entities_matches_official_table() {
        // Each row is `(input, official_decoded_value)` ground-truthed against
        // svelte@5.56.3 `decode_character_references(input, /*is_attr*/ true)`.
        // (The DECODE step only — re-escaping is a separate step the skeleton
        // serializer applies.) NOTE: `&#10` (line feed) → a space, `&#0` (falsy)
        // → kept literal `&#0`.
        let cases: &[(&str, &str)] = &[
            // Semicolonless numeric (W4) — `;` is optional.
            ("&#65", "A"),
            ("&#x41", "A"),
            ("&#65;", "A"),
            ("&#x41;", "A"),
            ("&#65B", "AB"),
            // Named longest-match + legacy boundary (W7).
            ("&copy;", "\u{00a9}"),
            ("&copy x", "\u{00a9} x"),
            ("&copy_x", "&copy_x"), // `_` is a word char → blocked, kept literal
            ("a&copy=b", "a&copy=b"), // `=` blocks → kept literal
            ("&copyx", "&copyx"),   // following alnum blocks → kept literal
            ("&copyright;", "&copyright;"), // `copy` blocked by `r`, full name unknown
            // UPPERCASE `X` PREFIX is NOT a numeric reference — the official
            // pattern `#(?:x[a-fA-F\d]+|\d+)(?:;)?` accepts a lowercase `x`
            // only (first-hand: `class="a&#X20;b"` keeps the literal,
            // markup-escaped `a&amp;#X20;b`). Hex DIGITS may still be
            // uppercase — only the prefix is case-sensitive.
            ("&#X41;", "&#X41;"),     // uppercase prefix kept literal
            ("&#X20;", "&#X20;"),     // the oracle's exact class-value case
            ("a&#X20;b", "a&#X20;b"), // embedded — no space is produced
            ("&#x4A1;", "\u{04A1}"),  // uppercase hex DIGITS still decode → ҡ
            // validate_code specifics.
            ("&#128;", "\u{20ac}"),     // Windows-1252 remap → €
            ("&#10;x", " x"),           // line feed → space
            ("&#x1F600;", "\u{1F600}"), // supplementary plane
            ("&#0;", "&#0;"),           // falsy code kept literal
            ("&#xD800;", "\u{0000}"),   // surrogate-half → NUL char
            // Unknown / bare.
            ("&bogus;", "&bogus;"), // unknown ref kept literal (no decode)
            ("Tom & Jerry", "Tom & Jerry"), // bare `&` kept literal
            // Adjacent named refs.
            ("&alpha;&beta;", "\u{03b1}\u{03b2}"),
        ];
        for (input, expected) in cases {
            let got = decode_attribute_entities_for_test(input);
            assert_eq!(
                &got, expected,
                "decode_attribute_entities({input:?}) must match official (got {got:?})"
            );
        }
    }

    #[test]
    fn decode_runs_exactly_once_amp_protects_inner_reference() {
        // DECODE-ONCE pin (ground-truthed against svelte@5.56.3
        // `decode_character_references('a&amp;#32;b', /*is_attr*/ true)`): the
        // `&amp;` decodes to `&` and the produced `&#32;` stays LITERAL — the
        // single pass never re-scans its own output, so the result is the ONE
        // token `a&#32;b`. A double decode would re-decode the produced
        // `&#32;` to a space and split the value into the TWO tokens `a b`
        // (the css scope matcher would then wrongly match `.b`).
        let once = decode_attribute_entities_for_test("a&amp;#32;b");
        assert_eq!(
            once, "a&#32;b",
            "`&amp;` decodes to `&`; the produced `#32;` stays literal (decode-once)"
        );
        assert_ne!(
            once, "a b",
            "a two-token `a b` result means the decoder re-decoded its own output (double-decode)"
        );
        // The double-decode result IS `a b` — the fixed point differs, so the
        // equality above discriminates one pass from two.
        assert_eq!(
            decode_attribute_entities_for_test(&once),
            "a b",
            "NON-VACUITY: a second pass over the decoded value yields `a b`, so \
             the decode-once assertion genuinely discriminates"
        );
    }
}

mod official_event_name_unit_table {
    use crate::svelte::runtime::events::{can_delegate_event, normalize_event_name};

    #[test]
    fn normalize_event_name_strips_capture_per_official_is_capture_event() {
        // `(raw_name_without_on, (normalized_name, is_capture))` ground-truthed
        // against svelte@5.56.3 `is_capture_event` + the slice(0,-7) strip.
        let cases: &[(&str, (&str, bool))] = &[
            ("click", ("click", false)),
            ("clickcapture", ("click", true)),
            ("input", ("input", false)),
            ("focus", ("focus", false)),
            // The two excluded names: they END in `capture` but are NOT capture
            // events (their name is kept whole).
            ("gotpointercapture", ("gotpointercapture", false)),
            ("lostpointercapture", ("lostpointercapture", false)),
            // The doubled form: the OUTER capture is stripped.
            ("gotpointercapturecapture", ("gotpointercapture", true)),
            ("lostpointercapturecapture", ("lostpointercapture", true)),
            ("pointerdowncapture", ("pointerdown", true)),
        ];
        for (raw, (name, cap)) in cases {
            let (got_name, got_cap) = normalize_event_name(raw);
            assert_eq!(
                (got_name.as_str(), got_cap),
                (*name, *cap),
                "normalize_event_name({raw:?}) must match official"
            );
        }
    }

    #[test]
    fn can_delegate_event_matches_official_set() {
        // The delegated set (DELEGATED_EVENTS) ground-truthed against
        // svelte@5.56.3 `can_delegate_event`.
        for name in [
            "beforeinput",
            "click",
            "change",
            "dblclick",
            "contextmenu",
            "focusin",
            "focusout",
            "input",
            "keydown",
            "keyup",
            "mousedown",
            "mousemove",
            "mouseout",
            "mouseover",
            "mouseup",
            "pointerdown",
            "pointermove",
            "pointerout",
            "pointerover",
            "pointerup",
            "touchend",
            "touchmove",
            "touchstart",
        ] {
            assert!(can_delegate_event(name), "{name} must be delegable");
        }
        // Negatives: non-bubbling / capture-suffixed raw names are NOT delegable.
        for name in [
            "focus",
            "blur",
            "mouseenter",
            "mouseleave",
            "clickcapture",
            "gotpointercapture",
            "scroll",
        ] {
            assert!(!can_delegate_event(name), "{name} must NOT be delegable");
        }
    }
}

// ===========================================================================
// EXACT node-path operation goldens. Reachability-only assertions cannot catch
// a W2-class separator/offset bug (an extra synthesized separator keeps a node
// "reachable" while shifting its sibling offset). These assert the PRECISE
// NodePathStep sequence a node's DOM walk produces, so a wrong offset FAILS.
// ===========================================================================

mod exact_node_path_goldens {
    use super::*;
    use crate::svelte::runtime::html::{NodePathPlan, NodePathStep, PathBase};

    fn plan_for(
        src: &str,
    ) -> (
        super::super::ir::SvelteRuntimeIr<'static>,
        Vec<NodePathPlan>,
    ) {
        // Leak the source/allocator so the returned IR can own 'static spans for
        // the test (test-only; the process exits after).
        let src: &'static str = Box::leak(src.to_string().into_boxed_str());
        let alloc: &'static Allocator = Box::leak(Box::new(Allocator::default()));
        let ir = lower(src, alloc);
        let paths = plan_static_templates(&ir, None).client_paths;
        (ir, paths)
    }

    /// The exact step sequence reaching the FIRST interpolation node in the root
    /// region, with its base.
    fn first_interp_path(src: &str) -> (PathBase, Vec<NodePathStep>) {
        let (ir, paths) = plan_for(src);
        let interp = ir
            .nodes
            .iter()
            .enumerate()
            .find_map(|(i, n)| matches!(n, IrNode::Interpolation { .. }).then_some(i))
            .expect("an interpolation node");
        let node = super::super::ir::NodeId(interp as u32);
        let path = paths
            .iter()
            .find(|p| p.node == node)
            .unwrap_or_else(|| panic!("no path reaches the interpolation for {src:?}"));
        (path.base, path.steps.clone())
    }

    #[test]
    fn dynamic_root_after_static_root_walks_first_child_then_sibling_one() {
        // `<a></a>{x}` — the `{x}` interpolation is root index 1 (after the static
        // `<a>`). The DOM walk is FirstChild (descend into the fragment) THEN
        // Sibling{1}. A bug that synthesizes an extra separator between the roots
        // would shift the interpolation to index 2 (Sibling{2}) — caught here.
        let (base, steps) = first_interp_path("<a></a>{x}");
        assert_eq!(
            base,
            PathBase::Fragment,
            "the path is rooted at the fragment"
        );
        assert_eq!(
            steps,
            vec![
                NodePathStep::FirstChild,
                NodePathStep::Sibling { offset: 1 }
            ],
            "the dynamic root after one static root is FirstChild then Sibling{{1}}"
        );
    }

    #[test]
    fn static_dynamic_static_interp_walks_sibling_one() {
        // `<a></a>{x}<c></c>` — the `{x}` is root index 1 (between the two
        // statics). The walk is FirstChild then Sibling{1}. A synthesized
        // separator would push it to index 2.
        let (base, steps) = first_interp_path("<a></a>{x}<c></c>");
        assert_eq!(base, PathBase::Fragment);
        assert_eq!(
            steps,
            vec![
                NodePathStep::FirstChild,
                NodePathStep::Sibling { offset: 1 }
            ],
            "the middle interpolation between two static roots is at Sibling{{1}}"
        );
    }

    #[test]
    fn nested_block_dynamic_interp_path_is_self_contained_first_child() {
        // `<div>{#if c}<p>{x}</p>{/if}</div>` — inside the if-branch region the
        // `{x}` is reached from the region's OWN fragment: FirstChild into the
        // `<p>`'s text. The nested region's walk is self-contained (not offset by
        // any root-region separator).
        let (ir, paths) =
            plan_for("<script>let c=true,x=1;</script><div>{#if c}<p>{x}</p>{/if}</div>");
        // The interpolation `{x}` node.
        let interp = ir
            .nodes
            .iter()
            .enumerate()
            .find_map(|(i, n)| matches!(n, IrNode::Interpolation { .. }).then_some(i))
            .expect("an interpolation node");
        let node = super::super::ir::NodeId(interp as u32);
        let path = paths
            .iter()
            .find(|p| p.node == node)
            .expect("a path reaches the nested interpolation");
        // The first step descends into the region fragment (the `<p>`), then into
        // its child text; the offset must be 0 (the `{x}` is the `<p>`'s first and
        // only child) — NOT shifted by any separator.
        assert!(
            path.steps
                .iter()
                .all(|s| !matches!(s, NodePathStep::Sibling { offset } if *offset > 1)),
            "the nested interpolation has no large sibling offset (self-contained region walk); got {:?}",
            path.steps
        );
        // The path must carry at least one descent step.
        assert!(
            path.steps
                .iter()
                .any(|s| matches!(s, NodePathStep::FirstChild | NodePathStep::Child { .. })),
            "the nested interpolation path descends from its region fragment; got {:?}",
            path.steps
        );
    }
}

/// Skeleton parity against official svelte@5.56.3 for the controlled-child
/// optimization, `{@html}`, attribute source-order/entities, and nested table
/// structure. Each expected skeleton was ground-truthed against the pinned
/// compiler; templates are compared as a sorted multiset because the template
/// DECLARATION ORDER is cosmetic (the matrix normalizes it the same way — both
/// regions exist with identical content and mount identically at runtime).
#[cfg(test)]
mod official_controlled_child_and_attr_skeleton {
    use super::*;

    /// The planner's `from_html` skeletons, sorted (declaration order is cosmetic).
    fn sorted_htmls(src: &str) -> Vec<String> {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        let mut v: Vec<String> = plan_static_templates(&ir, None)
            .templates
            .iter()
            .filter_map(|t| match t {
                TemplateFactory::FromHtml { html, .. } => Some(html.clone()),
                _ => None,
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn sole_each_child_is_controlled_no_anchor() {
        // svelte@5.56.3: a `{#each}` that is the SOLE child of an element is
        // CONTROLLED — the `<ul>` body is EMPTY (no `<!>` anchor), the each body is
        // its own `<li> </li>` region.
        assert_eq!(
            sorted_htmls(
                "<script>let items=$state([])</script><ul>{#each items as i}<li>{i}</li>{/each}</ul>"
            ),
            vec!["<li> </li>".to_string(), "<ul></ul>".to_string()],
        );
    }

    #[test]
    fn each_with_text_sibling_is_not_controlled_keeps_anchor() {
        // svelte@5.56.3: a `{#each}` with a sibling text (`x`) is NOT controlled —
        // the `<ul>` body is `x<!>` (the each keeps its `<!>` anchor).
        assert_eq!(
            sorted_htmls(
                "<script>let items=$state([])</script><ul>x{#each items as i}<li>{i}</li>{/each}</ul>"
            ),
            vec!["<li> </li>".to_string(), "<ul>x<!></ul>".to_string()],
        );
    }

    #[test]
    fn sole_html_tag_is_controlled_but_with_text_keeps_anchor() {
        // svelte@5.56.3: a sole `{@html}` is controlled (`<div></div>`); with a text
        // sibling it keeps its `<!>` anchor (`<div>x<!></div>`).
        assert_eq!(
            sorted_htmls("<script>let h=$state('')</script><div>{@html h}</div>"),
            vec!["<div></div>".to_string()],
        );
        assert_eq!(
            sorted_htmls("<script>let h=$state('')</script><div>x{@html h}</div>"),
            vec!["<div>x<!></div>".to_string()],
        );
    }

    #[test]
    fn sole_if_block_is_not_controlled_keeps_anchor() {
        // svelte@5.56.3: an `{#if}` (unlike `{#each}`/`{@html}`) is NOT controlled —
        // even as a sole child it keeps its `<!>` anchor (`<div><!></div>`).
        assert_eq!(
            sorted_htmls("<script>let c=$state(true)</script><div>{#if c}<p>x</p>{/if}</div>"),
            vec!["<div><!></div>".to_string(), "<p>x</p>".to_string()],
        );
    }

    #[test]
    fn static_attributes_keep_source_order_and_decode_then_reescape_entities() {
        // svelte@5.56.3: static attributes are emitted in SOURCE order; an authored
        // attribute entity round-trips through decode-then-reescape (`&quot;` → `"`
        // → `&quot;`).
        assert_eq!(
            sorted_htmls("<div class=\"a\" id=\"b\" data-x=\"c\"></div>"),
            vec!["<div class=\"a\" id=\"b\" data-x=\"c\"></div>".to_string()],
        );
        assert_eq!(
            sorted_htmls("<div title=\"a&quot;b\"></div>"),
            vec!["<div title=\"a&quot;b\"></div>".to_string()],
        );
    }

    #[test]
    fn nested_table_structure_serializes_verbatim() {
        // svelte@5.56.3: a well-formed nested table serializes structurally verbatim
        // (no spurious whitespace text nodes between the structural elements).
        assert_eq!(
            sorted_htmls("<table><tbody><tr><td>x</td></tr></tbody></table>"),
            vec!["<table><tbody><tr><td>x</td></tr></tbody></table>".to_string()],
        );
    }

    #[test]
    fn pre_and_textarea_preserve_whitespace_verbatim() {
        // svelte@5.56.3: a `<pre>` / `<textarea>` preserves ALL of its whitespace
        // (leading, trailing, and interior runs) — `preserve_whitespace = true` —
        // while a `<p>` trims edges. FAILS against a planner that always trims.
        assert_eq!(
            sorted_htmls("<pre>  a   b  </pre>"),
            vec!["<pre>  a   b  </pre>".to_string()],
            "a <pre> preserves leading/trailing/interior whitespace"
        );
        assert_eq!(
            sorted_htmls("<textarea>  x  </textarea>"),
            vec!["<textarea>  x  </textarea>".to_string()],
            "a <textarea> preserves whitespace"
        );
        // Contrast: a <p> still trims edges (interior preserved).
        assert_eq!(
            sorted_htmls("<p>  a   b  </p>"),
            vec!["<p>a   b</p>".to_string()],
            "a <p> trims edge whitespace (interior preserved)"
        );
    }

    #[test]
    fn pre_whitespace_preservation_is_inherited_by_descendants() {
        // svelte@5.56.3: the `preserve_whitespace` flag is INHERITED — whitespace
        // inside a `<code>` nested in a `<pre>` is preserved too.
        assert_eq!(
            sorted_htmls("<pre>  <code>  x  </code>  </pre>"),
            vec!["<pre>  <code>  x  </code>  </pre>".to_string()],
            "whitespace preservation propagates into a <pre>'s descendants"
        );
    }

    /// The fragment flag (bitmask) of the single template factory.
    fn flag_bits(src: &str) -> Option<u8> {
        let alloc = Allocator::default();
        let ir = lower(src, &alloc);
        match &plan_static_templates(&ir, None).templates[0] {
            TemplateFactory::FromHtml { fragment_flag, .. } => fragment_flag.map(|f| f.bits()),
            other => panic!("expected a from_html factory, got {other:?}"),
        }
    }

    #[test]
    fn custom_element_sets_use_import_node_flag() {
        // svelte@5.56.3: a CUSTOM element (`<my-widget>`) needs `importNode`, so a
        // SINGLE custom-element root gets flag TEMPLATE_USE_IMPORT_NODE (2) — NOT
        // 1 (no fragment) and NOT absent. FAILS against a planner that only sets the
        // multi-root fragment bit.
        assert_eq!(
            flag_bits("<my-widget></my-widget>"),
            Some(TemplateFlag::USE_IMPORT_NODE),
            "a single custom-element root sets the import-node flag (2)"
        );
        // A custom element nested in a plain element also sets it (template-wide).
        assert_eq!(
            flag_bits("<div><my-el></my-el></div>"),
            Some(TemplateFlag::USE_IMPORT_NODE),
            "a custom element anywhere in the template sets the import-node flag"
        );
    }

    #[test]
    fn video_element_sets_use_import_node_flag() {
        // svelte@5.56.3: a `<video>` also needs `importNode` (flag 2).
        assert_eq!(
            flag_bits("<video></video>"),
            Some(TemplateFlag::USE_IMPORT_NODE),
            "a single <video> root sets the import-node flag (2)"
        );
    }

    #[test]
    fn is_attribute_marks_a_customized_built_in_element() {
        // svelte@5.56.3: an element with an `is="…"` attribute is a customized
        // built-in (a custom element form) → import-node flag.
        assert_eq!(
            flag_bits("<button is=\"my-button\">x</button>"),
            Some(TemplateFlag::USE_IMPORT_NODE),
            "an `is=` attribute marks a custom element (import-node flag)"
        );
    }

    #[test]
    fn multi_root_with_custom_element_combines_fragment_and_import_node_bits() {
        // svelte@5.56.3: two custom-element roots → TEMPLATE_FRAGMENT (1) |
        // TEMPLATE_USE_IMPORT_NODE (2) = 3. A plain multi-root stays 1.
        assert_eq!(
            flag_bits("<my-a></my-a><my-b></my-b>"),
            Some(TemplateFlag::FRAGMENT | TemplateFlag::USE_IMPORT_NODE),
            "two custom-element roots combine the fragment and import-node bits (3)"
        );
        assert_eq!(
            flag_bits("<a></a><b></b>"),
            Some(TemplateFlag::FRAGMENT),
            "a plain multi-root stays just the fragment bit (1)"
        );
    }

    #[test]
    fn plain_single_root_has_no_flag() {
        // svelte@5.56.3: a single plain-HTML root has flag 0 (no trailing argument).
        assert_eq!(
            flag_bits("<div></div>"),
            None,
            "a single plain-HTML root has no trailing flag"
        );
    }

    #[test]
    fn custom_element_attributes_are_dropped_from_skeleton_except_is() {
        // svelte@5.56.3: a CUSTOM element sets its attributes via PROPERTIES at
        // runtime, so they are NOT in the static skeleton — `<my-widget label="x"
        // foo="y">` → `<my-widget></my-widget>`. The `is` attribute is the one
        // exception (kept for the customized-built-in upgrade): `<div is="my-div"
        // class="c">` → `<div is="my-div"></div>`. A `<video>` is NOT a custom
        // element, so its attributes stay. FAILS against a planner that serializes a
        // custom element's static attributes.
        assert_eq!(
            sorted_htmls("<my-widget label=\"x\" foo=\"y\"></my-widget>")
                .into_iter()
                .next()
                .unwrap(),
            "<my-widget></my-widget>",
            "a dash-named custom element drops ALL its static attributes"
        );
        assert_eq!(
            sorted_htmls("<div is=\"my-div\" class=\"c\"></div>")
                .into_iter()
                .next()
                .unwrap(),
            "<div is=\"my-div\"></div>",
            "an `is=` customized built-in keeps ONLY the `is` attribute"
        );
        assert_eq!(
            sorted_htmls("<video src=\"x.mp4\" controls></video>")
                .into_iter()
                .next()
                .unwrap(),
            "<video src=\"x.mp4\" controls=\"\"></video>",
            "a <video> is not a custom element — its attributes stay in the skeleton"
        );
    }

    #[test]
    fn empty_string_class_attribute_is_dropped() {
        // svelte@5.56.3: a static `class=""` (the EXACTLY empty string) is DROPPED
        // (`<div class="">` → `<div>`). A `class=" "` (a space) is KEPT, and a
        // non-empty/other-name empty attr is kept. FAILS against a planner that
        // always emits the attribute.
        assert_eq!(
            sorted_htmls("<div class=\"\"></div>"),
            vec!["<div></div>".to_string()],
            "a static empty-string class is dropped"
        );
        // `class=" "` (a space) is NOT the empty string → kept.
        assert_eq!(
            sorted_htmls("<div class=\" \"></div>"),
            vec!["<div class=\" \"></div>".to_string()],
            "a class with a space value is kept (not the empty string)"
        );
        // An empty-string NON-class attribute is kept (the drop is class-specific).
        assert_eq!(
            sorted_htmls("<div id=\"\"></div>"),
            vec!["<div id=\"\"></div>".to_string()],
            "an empty-string non-class attribute is kept"
        );
    }

    #[test]
    fn every_state_binding_carries_a_classification() {
        // F8 invariant: `prepare_state_bindings` classifies EVERY top-level `$state`
        // binding, so the `$state` emitter's missing-classification arm is provably
        // dead for well-formed input (no silent `$.state(...)` default fallback).
        // Discriminating: a `$state` binding with `state: None` would surface here.
        let alloc = Allocator::default();
        let ir = lower(
            "<script>\n\
                let a = $state(0);\n\
                let b = $state({ x: 1 });\n\
                let c = $state('s');\n\
            </script>\n<button onclick={() => { a++; b.x++; }}>{a} {b.x} {c}</button>\n",
            &alloc,
        );
        let state_bindings: Vec<_> = ir
            .analysis
            .bindings
            .all()
            .iter()
            .filter(|b| {
                matches!(
                    b.kind,
                    BindingRuntimeKind::StateSignal { .. }
                        | BindingRuntimeKind::StateProxy
                        | BindingRuntimeKind::BareProxy
                )
            })
            .collect();
        assert!(
            !state_bindings.is_empty(),
            "the fixture declares $state bindings"
        );
        for b in state_bindings {
            assert!(
                b.state.is_some(),
                "every $state binding must carry a classification (no silent \
                 missing-classification fallback): {b:?}"
            );
        }
    }
}
