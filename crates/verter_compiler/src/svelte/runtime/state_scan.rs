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

/// Collect the top-level `$derived` / `$props` / `$bindable` /
/// `$effect.tracking` binding declarations of a script program, returning
/// `(name, kind)` rows in source order.
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
/// - a declaration matching the item carrier's EXACT effect-rune-init shape
///   (the shared [`super::instance_items::effect_rune_init_shape`] predicate:
///   `let`/`const`, one plain non-`$`-prefixed identifier declarator, no TS
///   annotation, a well-formed `$effect.tracking()` init) → the name is
///   [`BindingRuntimeKind::EffectTrackingConst`] — a PLAIN one-shot value read
///   bare (never `$.get`), whose template/attribute read joins the region's
///   `$.template_effect` (official cannot static-fold a call-init const). A
///   declaration the carrier REFUSES (`var`, multi-declarator, TS-annotated,
///   `$`-prefixed, malformed call) mints NO fact — the minting is never broader
///   than the carrier.
/// - a declaration matching the `$props.id()` declarator carrier's shape (the
///   shared [`super::instance_items::props_id_decl_shape`] predicate) → the id
///   name is [`BindingRuntimeKind::PropsIdConst`] — the same plain one-shot
///   call-init-const read discipline (the hoisted `const <name> = $.props_id();`);
///   its literal-only SIBLING declarators mint nothing (plain locals).
///
/// Only DIRECT top-level declarators whose initializer is one of these rune
/// calls are returned (a shadowing local is excluded by the structural callee
/// match; an each/snippet binding never reaches this top-level declarator scan).
#[must_use]
pub fn collect_rune_bindings<'a>(program: &Program<'a>) -> Vec<(String, BindingRuntimeKind)> {
    use super::expr::EffectFamilyCallKind;
    let mut out = Vec::new();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        // The effect-rune-init shapes consult the item carrier's shared
        // declaration-shape predicate, so a carrier-refused declaration mints no
        // fact. A matching declaration has exactly one root/tracking-init
        // declarator, so no other arm below can also apply to it. An
        // `EffectRoot` init mints NO binding fact (the result local is a plain
        // non-reactive teardown binding).
        if let Some(shape) = super::instance_items::effect_rune_init_shape(decl) {
            if shape.kind == EffectFamilyCallKind::EffectTracking {
                out.push((shape.name, BindingRuntimeKind::EffectTrackingConst));
            }
            continue;
        }
        // The `$props.id()` declarator carrier — the id binding is a PLAIN
        // one-shot `PropsIdConst` (read bare, never `$.get`; template reads join
        // the region's `$.template_effect`). Consults the item carrier's SHARED
        // declaration-shape predicate, so a carrier-refused declaration mints no
        // fact; the literal-only SIBLING declarators mint nothing (plain locals).
        if let Some(shape) = super::instance_items::props_id_decl_shape(decl) {
            out.push((shape.name, BindingRuntimeKind::PropsIdConst));
            continue;
        }
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

/// Declare each top-level `export let <ident>` declarator of the instance script
/// as a [`BindingRuntimeKind::Prop`] binding in `scope` — the LEGACY prop
/// surface (`export let label` lowers through `$.prop($$props, 'label', 8)` and
/// ALWAYS reads as the accessor call). Registered mode-independently, like the
/// rune bindings: a RUNES-mode `export let` is an official compile error
/// (`legacy_export_invalid`) rejected before these bindings are ever consumed.
/// Destructured export-let patterns register nothing (they fail closed at the
/// instance-script item allowlist), as does `export var` / `export const` (each
/// a distinct fail-closed surface).
pub(super) fn prepare_legacy_export_prop_bindings(
    instance_source: Option<&str>,
    alloc: &Allocator,
    scope: ScopeId,
    scopes: &mut ScopeGraph,
    bindings: &mut BindingTable,
) {
    let Some(text) = instance_source else {
        return;
    };
    let Some(program) = reparse_module(alloc, text) else {
        return;
    };
    for stmt in &program.body {
        let Statement::ExportNamedDeclaration(export) = stmt else {
            continue;
        };
        let Some(oxc_ast::ast::Declaration::VariableDeclaration(decl)) = &export.declaration else {
            continue;
        };
        if decl.kind != oxc_ast::ast::VariableDeclarationKind::Let {
            continue;
        }
        for d in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &d.id else {
                continue;
            };
            let name = id.name.as_str();
            // A name already declared in the scope is never re-registered (valid
            // source cannot redeclare, so this is purely defensive).
            if scopes.resolve(bindings, scope, name).is_some() {
                continue;
            }
            let binding = bindings.push(BindingInfo {
                name: name.to_string(),
                scope,
                kind: BindingRuntimeKind::Prop,
                state: None,
            });
            scopes.declare(scope, name, binding);
        }
    }
}

/// Declare every ADMITTED top-level static import LOCAL of one slot as its typed
/// NON-REACTIVE import binding in `scope` — a default `.svelte`-COMPONENT import as
/// [`BindingRuntimeKind::ComponentImport`] (so a `<Child/>` invocation's static
/// callee resolves to the import and reads emit the bare callee, never `$.get`),
/// and every other imported local (named / aliased / namespace / non-`.svelte`
/// default) as the non-writable [`BindingRuntimeKind::ImportedValue`].
///
/// `imports` is the slot's ADMITTED slice of the SHARED per-component
/// [`ClassifiedScriptImports`](super::client_surface_imports::ClassifiedScriptImports)
/// carrier (classified ONCE at IR construction) — the same carriers the module
/// prelude emits, so the bindings template reads resolve against are exactly the
/// emitted imports; the kind mapping is the shared
/// [`import_specifier_binding_kind`](super::client_surface_imports::import_specifier_binding_kind)
/// rail. A slot whose classification was refused passes the EMPTY slice (no
/// bindings) — the surface classifier propagates the retained refusal and the
/// component fails closed before any binding is consumed. Called per slot: the
/// module script declares into the MODULE scope, the instance script into the root
/// scope.
pub(super) fn prepare_import_bindings(
    imports: &[super::client_imports::UserImport],
    scope: ScopeId,
    scopes: &mut ScopeGraph,
    bindings: &mut BindingTable,
) {
    for import in imports {
        for (local, kind) in super::client_surface_imports::import_binding_entries(import) {
            let binding = bindings.push(BindingInfo {
                name: local.to_string(),
                scope,
                kind,
                state: None,
            });
            scopes.declare(scope, local, binding);
        }
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
/// shared [`ScopeAwareRuneDetector`] in [`super::rune_scan`], which reuses the same
/// lexical-scope `ShadowStack` model the other syntax-side collectors use.
///
/// `store_exempt` is the rune-root ACCESSOR exemption set
/// ([`rune_root_accessor_exemptions`](super::store_subscriptions::rune_root_accessor_exemptions)):
/// a `$state` whose base `state` is a declared store candidate is a STORE
/// ACCESSOR reference, not a rune use — official deletes store-classified
/// names from the reference set BEFORE the `some(is_rune)` inference, so such
/// a component stays in LEGACY mode.
#[must_use]
pub fn script_uses_runes(
    alloc: &Allocator,
    text: &str,
    store_exempt: &rustc_hash::FxHashSet<String>,
) -> bool {
    let Some(program) = reparse_module(alloc, text) else {
        return false;
    };
    let mut detector = ScopeAwareRuneDetector::with_store_exemptions(store_exempt.clone());
    detector.visit_program(&program);
    detector.used()
}

/// Whether the instance script contains a DEFINITIVELY-LEGACY top-level construct
/// — the negated half of the official `analysis.maybe_runes` condition
/// (svelte@5.56.3 `phases/2-analyze/index.js`): a top-level `LabeledStatement`
/// (any label, exactly as official checks the raw AST — the admitted surface only
/// ever carries `$:`) or an `export let` (a `let`-kind export declaration, or an
/// export specifier whose local resolves to a top-level `let`). A component with
/// one of these can never be "wannabe runes", so the legacy value wrap applies.
///
/// A malformed instance parse returns `false` — the malformed-script diagnostic
/// path owns that failure; the mode facts stay inert.
#[must_use]
pub fn instance_forces_definite_legacy(alloc: &Allocator, text: &str) -> bool {
    let Some(program) = reparse_module(alloc, text) else {
        return false;
    };
    // Top-level `let` names — the export-specifier resolution set
    // (`export { x }` over a top-level `let x`).
    let mut let_names: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for stmt in &program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            if decl.kind == oxc_ast::ast::VariableDeclarationKind::Let {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    let_names.extend(names);
                }
            }
        }
    }
    for stmt in &program.body {
        match stmt {
            Statement::LabeledStatement(_) => return true,
            Statement::ExportNamedDeclaration(export) => {
                if let Some(oxc_ast::ast::Declaration::VariableDeclaration(decl)) =
                    &export.declaration
                {
                    if decl.kind == oxc_ast::ast::VariableDeclarationKind::Let {
                        return true;
                    }
                }
                if export.specifiers.iter().any(|spec| {
                    matches!(
                        &spec.local,
                        oxc_ast::ast::ModuleExportName::IdentifierReference(id)
                            if let_names.contains(id.name.as_str())
                    )
                }) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
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

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    /// The names minted as [`BindingRuntimeKind::EffectTrackingConst`] for an
    /// instance-script `source`.
    fn tracking_bindings(source: &str) -> Vec<String> {
        let alloc = Allocator::default();
        let program = reparse_module(&alloc, source).expect("test source parses");
        collect_rune_bindings(&program)
            .into_iter()
            .filter(|(_, kind)| *kind == BindingRuntimeKind::EffectTrackingConst)
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn tracking_const_minting_matches_the_item_carrier_shape() {
        // The carrier-accepted declaration shapes mint the fact (both keywords).
        assert_eq!(tracking_bindings("const t = $effect.tracking();"), ["t"]);
        assert_eq!(tracking_bindings("let t = $effect.tracking();"), ["t"]);
        // Every carrier-REJECTED declaration shape mints NO fact — the minting
        // shares the item carrier's EXACT declaration-shape predicate
        // (`effect_rune_init_shape`). A fact minted for a shape the carrier
        // refuses would leave a half-classified binding behind the refusal
        // (`var` read semantics, multi-declarator mixing, TS-annotated and
        // `$`-prefixed declarators are all carrier refusals).
        assert!(
            tracking_bindings("var t = $effect.tracking();").is_empty(),
            "a `var` declarator must mint no tracking fact"
        );
        assert!(
            tracking_bindings("let a = $effect.tracking(), b = 0;").is_empty(),
            "a multi-declarator declaration must mint no tracking fact"
        );
        assert!(
            tracking_bindings("const t: boolean = $effect.tracking();").is_empty(),
            "a TS-annotated declarator must mint no tracking fact"
        );
        assert!(
            tracking_bindings("const $t = $effect.tracking();").is_empty(),
            "a `$`-prefixed declarator must mint no tracking fact"
        );
        // A malformed init (the zero-arg contract rejects an argument) mints
        // nothing either — form and shape are both carrier-shared.
        assert!(
            tracking_bindings("const t = $effect.tracking(1);").is_empty(),
            "a malformed tracking call must mint no fact"
        );
    }
}
