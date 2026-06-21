//! Instance-script rune declaration scanning + rune-use detection.
//!
//! This is the SYNTAX-side scan over a reparsed instance/module program. It owns
//! the rune-binding CLASSIFICATION: the top-level `$state` / `$state.raw`
//! declarations (with their `should_proxy` proxiability resolved via the one-hop
//! identifier follow), and the other reactive runes — `$derived` / `$derived.by`
//! (Derived), `$props()` destructures (Prop / BindableProp via `$bindable`). It
//! also detects whether a script uses any Svelte 5 rune. It drives ONLY
//! classification inputs — it never resolves types or emits JS. The `$state`
//! proxy/signal LOWERING decision itself lives in
//! [`super::expr::classify_state_lowering`].

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Expression, Program, Statement};
use oxc_ast_visit::Visit;
use rustc_hash::FxHashMap;

use super::expr::{
    collect_pattern_names, expr_is_proxiable, init_is_proxiable, is_bindable_call,
    is_derived_callee, is_props_callee, reparse_module, state_rune_call, BindingInfo,
    BindingRuntimeKind, BindingTable, ProxyInit, ScopeGraph, ScopeId, ScriptUseCollector,
    StateRuneKind,
};
use super::rune_scan::ScopeAwareRuneDetector;

/// Collect the top-level `$state` / `$state.raw` binding declarations of an
/// instance-script program, returning `(name, declared, proxiable)` rows. Only
/// direct top-level `let`/`const`/`var` declarators whose initializer is a
/// `$state` family call are returned. `proxiable` is the official `should_proxy`
/// predicate over the initializer (with the one-hop identifier follow resolved
/// against the program's own top-level bindings).
#[must_use]
pub fn collect_state_declarations<'a>(program: &Program<'a>) -> Vec<(String, StateRuneKind, bool)> {
    // Build the proxy-init map first (the identifier follow needs it).
    let scope_inits = collect_proxy_inits(program);
    let mut out = Vec::new();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        // `let`/`const`/`var` — all top-level state declarators.
        for d in &decl.declarations {
            let Some(init) = &d.init else { continue };
            let Expression::CallExpression(call) = init else {
                continue;
            };
            let Some(declared) = state_rune_call(call) else {
                continue;
            };
            // The binding name (a plain identifier declarator).
            if let BindingPattern::BindingIdentifier(id) = &d.id {
                out.push((
                    id.name.to_string(),
                    declared,
                    init_is_proxiable(call, &scope_inits),
                ));
            }
        }
    }
    out
}

/// Collect the top-level `$derived` / `$props` / `$bindable` binding declarations
/// of a script program, returning `(name, kind)` rows in source order.
///
/// This is the non-`$state` rune binding classification (`$state` has its own
/// write-gated lowering path via [`collect_state_declarations`]). The kinds are
/// FIXED at declaration (no write-gating):
///
/// - `let X = $derived(e)` / `let X = $derived.by(fn)` → `X` is
///   [`BindingRuntimeKind::Derived`].
/// - `let <pattern> = $props()` → each destructured name is
///   [`BindingRuntimeKind::Prop`], EXCEPT a member with a `$bindable(…)` default,
///   which is [`BindingRuntimeKind::BindableProp`]; a rest and a whole-object
///   identifier are plain `Prop`s.
///
/// Only DIRECT top-level `let`/`const`/`var` declarators whose initializer is one
/// of these rune calls are returned (a shadowing local is excluded by the
/// structural callee match; an each/snippet binding never reaches this top-level
/// declarator scan).
#[must_use]
pub fn collect_rune_bindings<'a>(program: &Program<'a>) -> Vec<(String, BindingRuntimeKind)> {
    let mut out = Vec::new();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let Some(init) = &d.init else { continue };
            let Expression::CallExpression(call) = init else {
                continue;
            };
            if is_derived_callee(&call.callee) {
                // A `$derived` binding is a plain-identifier declarator.
                if let BindingPattern::BindingIdentifier(id) = &d.id {
                    out.push((id.name.to_string(), BindingRuntimeKind::Derived));
                }
            } else if is_props_callee(&call.callee) {
                // A `$props()` declarator: classify each declared name (Prop, or
                // BindableProp for a `$bindable(…)` default).
                for (name, is_bindable) in props_pattern_binding_kinds(&d.id) {
                    let kind = if is_bindable {
                        BindingRuntimeKind::BindableProp
                    } else {
                        BindingRuntimeKind::Prop
                    };
                    out.push((name, kind));
                }
            }
        }
    }
    out
}

/// Declare a script's non-`$state` rune bindings (`$derived` → Derived, `$props()`
/// destructures → Prop / BindableProp) in `scope`, entering each into the scope
/// graph + binding table so a scope-aware template read resolves to the right
/// kind.
///
/// Unlike `$state`, these kinds are FIXED at declaration — there is no
/// write-gated finalization — so this returns no tracking data. A same-name
/// `$state` binding in the same script is impossible (a name binds once), so the
/// declaration order relative to the `$state` binding preparation is immaterial.
pub fn prepare_rune_bindings(
    script_source: Option<&str>,
    alloc: &Allocator,
    scope: ScopeId,
    scopes: &mut ScopeGraph,
    bindings: &mut BindingTable,
) {
    let Some(text) = script_source else {
        return;
    };
    let Some(program) = reparse_module(alloc, text) else {
        return;
    };
    for (name, kind) in collect_rune_bindings(&program) {
        let binding = bindings.push(BindingInfo {
            name: name.clone(),
            scope,
            kind,
            state: None,
        });
        scopes.declare(scope, &name, binding);
    }
}

/// Build the one-hop proxy-init map for the program's TOP-LEVEL `let`/`const`/
/// `var` bindings: each binding name → its [`ProxyInit`] (scope-aware reassignment,
/// initializer proxiability, followability).
///
/// The `reassigned` fact is collected SCOPE-AWARELY through the shared
/// [`ScriptUseCollector`] [`ShadowStack`](super::expr) model: a write counts only
/// when it RESOLVES to the top-level binding, so a reassignment of an INNER
/// shadowed local of the same name (`let base=5; fn f(){ let base=6; base=7 }`)
/// does NOT mark the top-level `base` reassigned — and therefore does NOT block the
/// one-hop proxy follow for `let x=$state(base)`. (A reassignment of the top-level
/// binding itself DOES block the follow, matching `should_proxy`'s
/// `!binding.reassigned` guard.)
pub(super) fn collect_proxy_inits(program: &Program<'_>) -> FxHashMap<String, ProxyInit> {
    // The top-level declarator names whose reassignment we must resolve
    // scope-awarely (the candidate follow targets).
    let mut top_level_names = Vec::new();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            if let BindingPattern::BindingIdentifier(id) = &d.id {
                top_level_names.push(id.name.to_string());
            }
        }
    }

    // The scope-aware reassignment scan: track every top-level name through the
    // shared lexical ShadowStack so an inner shadowed write is never attributed to
    // the outer binding.
    let mut collector = ScriptUseCollector::tracking(&top_level_names);
    collector.visit_program(program);

    let mut map = FxHashMap::default();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &d.id else {
                continue;
            };
            let name = id.name.to_string();
            // A declarator with an EXPRESSION initializer is followable (function/
            // class declarations are statements, not declarators; an import is not
            // a declarator). An each/snippet binding never reaches this top-level
            // declarator scan.
            let (followable, init_proxiable) = match &d.init {
                Some(init) => (true, expr_is_proxiable(init, None)),
                None => (false, true),
            };
            map.insert(
                name.clone(),
                ProxyInit {
                    reassigned: collector.use_set(&name).reassigned,
                    init_proxiable,
                    followable,
                },
            );
        }
    }
    map
}

/// Whether an instance/module script body USES any Svelte 5 rune — a SCOPE-AWARE
/// structural (OXC identifier-reference) detection.
///
/// This mirrors the official runes-mode detection
/// (`phases/2-analyze/index.js`: `Array.from(scope.references.keys()).some(is_rune)`
/// over the binder-pruned reference set, where `get_global_keypath` returns null
/// when the rune name resolves to a declared binding). A rune name that is
/// SHADOWED by a local — most importantly a function PARAMETER named `$state`
/// (`function f($state){ return $state }`) — does NOT count, so such a component
/// stays in LEGACY mode. A rune name inside a string / comment is not an
/// identifier reference, so it never mis-classifies. The detection delegates to the
/// shared [`ScopeAwareRuneDetector`] in [`super::expr`], which reuses the same
/// lexical-scope `ShadowStack` model the other syntax-side collectors use.
#[must_use]
pub fn script_uses_runes(alloc: &Allocator, text: &str) -> bool {
    let Some(program) = reparse_module(alloc, text) else {
        return false;
    };
    let mut detector = ScopeAwareRuneDetector::default();
    detector.visit_program(&program);
    detector.used()
}

/// The runtime kind of each binding a `$props()` declarator pattern introduces, as
/// `(name, is_bindable)` rows in source order. `is_bindable` is `true` only for a
/// destructured member whose default initializer is a `$bindable(…)` call.
///
/// Every declared name is a [`BindingRuntimeKind::Prop`] EXCEPT a destructured
/// member whose default initializer is a `$bindable(…)` call, which is a
/// [`BindingRuntimeKind::BindableProp`]. A rest (`…rest`) and a whole-object
/// identifier are plain `Prop`s. This is the structural classification svelte
/// applies (a `$bindable` default lowers to the bindable `$.prop` flag; every
/// other prop lowers to a plain `$.prop` / `$.rest_props`).
fn props_pattern_binding_kinds(pattern: &BindingPattern<'_>) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    collect_props_binding_kinds(pattern, &mut out);
    out
}

/// Recursive worker for [`props_pattern_binding_kinds`].
fn collect_props_binding_kinds(pattern: &BindingPattern<'_>, out: &mut Vec<(String, bool)>) {
    match pattern {
        // A whole-object / plain identifier binding (`let p = $props()`) is a Prop.
        BindingPattern::BindingIdentifier(id) => out.push((id.name.to_string(), false)),
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                // A member with a `$bindable(…)` default is a BindableProp; the
                // member name is the LEFT of the assignment pattern.
                if let BindingPattern::AssignmentPattern(assign) = &prop.value {
                    if is_bindable_call(&assign.right) {
                        // The bindable name(s) — typically a single identifier.
                        let mut names = Vec::new();
                        collect_pattern_names(&assign.left, &mut names);
                        for n in names {
                            out.push((n, true));
                        }
                        continue;
                    }
                }
                // Any other member (plain or non-bindable default) is a plain Prop.
                collect_props_binding_kinds(&prop.value, out);
            }
            // A `…rest` is a plain Prop (the rest-props object).
            if let Some(rest) = &obj.rest {
                let mut names = Vec::new();
                collect_pattern_names(&rest.argument, &mut names);
                for n in names {
                    out.push((n, false));
                }
            }
        }
        // An array destructure of `$props()` is unusual; classify its names as Prop.
        BindingPattern::ArrayPattern(arr) => {
            for el in arr.elements.iter().flatten() {
                collect_props_binding_kinds(el, out);
            }
            if let Some(rest) = &arr.rest {
                let mut names = Vec::new();
                collect_pattern_names(&rest.argument, &mut names);
                for n in names {
                    out.push((n, false));
                }
            }
        }
        // A top-level default (`let x = … = $props()`) — descend to the left.
        BindingPattern::AssignmentPattern(assign) => {
            collect_props_binding_kinds(&assign.left, out);
        }
    }
}
