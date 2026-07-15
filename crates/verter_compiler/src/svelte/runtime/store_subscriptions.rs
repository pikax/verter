//! The client-runtime `$store` auto-subscription substrate.
//!
//! Owns the store-subscription facts the client pipeline consumes, all driven
//! from the typed OXC AST, the analyzed template-expression arena, and the
//! shared import carrier — never a source text scan, never an
//! import-source/type gate (a hand-rolled local `{subscribe, set}` factory
//! lowers IDENTICALLY to an imported `writable`):
//!
//! 1. [`store_base_candidates`] — the declared names a `$name` reference may
//!    legally subscribe to: every top-level instance-script declaration whose
//!    init is NOT a rune call, plus every ADMITTED import local (both slots,
//!    read from the single
//!    [`ClassifiedScriptImports`](super::client_surface_imports::ClassifiedScriptImports)
//!    carrier). Base RESOLUTION decides store-vs-rune — a rune-root ACCESSOR
//!    name (`$state` over a declared `const state = writable(0)`) IS a store
//!    subscription (official emits it in every mode); only a base whose own
//!    declarator init is a rune call (`let state = $state(0)`), the official
//!    `$derived`-from-`'svelte/store'` import special case, and a
//!    `$`-prefixed base are excluded.
//! 2. [`prepare_store_subscription_bindings`] — declares one
//!    [`BindingRuntimeKind::StoreSubscription`] binding per candidate (`$name`)
//!    in the instance root scope, so interpolation classification and the
//!    expression rewriter resolve `$name` scope-awarely.
//! 3. [`collect_store_subscriptions`] — the SCOPE-AWARE, first-seen-ordered
//!    subscription scan: the instance program (a lexical frame stack over the
//!    real declared names), then every ANALYZED template expression through
//!    its real [`AnalyzedExpr::scope`] + the shared [`ScopeGraph`] — never a
//!    reparse of the expression source. A `$NAME` whose BASE resolves to a
//!    NON-top-level binding (a `{#each as x}` alias, a `{#snippet}` param, an
//!    `{#await then x}` binding, a function parameter) is the official
//!    `store_invalid_scoped_subscription` COMPILE ERROR and rejects
//!    fail-closed; a `$NAME` shadowed by a local of the same literal `$name`
//!    (the `derived(a, ($a) => …)` callback param) is a plain local read;
//!    `$$`-prefixed names never subscribe.
//! 4. [`store_dependency_closure`] — the demand-driven top-level admission
//!    closure seeded by the subscribed bases: a subscribed `const` admits the
//!    top-level `const`s / `function`s its init (or body) freely references,
//!    transitively (`const doubled = derived(a, …)` admits `a`; `const c =
//!    w(0)` admits the factory `w`). Nothing is admitted without a
//!    subscription — an arbitrary `const x = make()` stays refused.
//!
//! This is the CLIENT runtime path only — the IDE store scanner
//! (`svelte/ide/store_scan.rs`) is a separate codegen path and is never
//! consulted here.

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, BlockStatement, CatchClause, Expression,
    ForInStatement, ForOfStatement, ForStatement, Function, Program, Statement,
    VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use rustc_hash::{FxHashMap, FxHashSet};

use super::client_imports::UserImportSlot;
use super::client_surface_imports::{import_binding_entries, ClassifiedScriptImports};
use super::expr::{
    arrow_scope_names, block_scope_names, collect_direct_decls, collect_pattern_names,
    collect_var_hoists, for_left_names, function_scope_names, AnalyzedExpr, BindingInfo,
    BindingRuntimeKind, BindingTable, ScopeGraph, ScopeId,
};
use super::rune_scan::RUNE_ROOT_NAMES;
use super::unsupported::UnsupportedSvelteRuntimeSurface;
use verter_span::Span;

/// One classified `$store` subscription: the accessor NAME (`$count`) whose
/// BASE (`count`) is a declared store candidate. Ordered facts drive the
/// per-store accessor emission (`const $count = () => $.store_get(count,
/// '$count', $$stores);`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoreSubscription {
    /// The `$`-prefixed accessor name (`$count`).
    pub(super) name: String,
}

impl StoreSubscription {
    /// The subscription's BASE binding name (`count` for `$count`).
    pub(super) fn base(&self) -> &str {
        &self.name[1..]
    }
}

/// Whether `name` is a candidate store-subscription ACCESSOR name: `$`-prefixed,
/// not `$$`-prefixed (the compiler-magic namespace), and longer than the bare
/// `$`. A RUNE-ROOT name is NOT excluded here — official decides store-vs-rune
/// by BASE RESOLUTION (`const state = writable(0)` + `{$state}` subscribes in
/// every mode), so the shape check stays name-agnostic and the candidate set
/// carries the rune/store discrimination.
fn is_subscription_shaped(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.first() == Some(&b'$') && bytes.len() > 1 && bytes.get(1) != Some(&b'$')
}

/// Whether a declarator INIT expression is a RUNE call (`$state(…)` /
/// `$state.raw(…)` / `$derived(…)` / `$derived.by(…)` / `$props()` /
/// `$props.id()` / `$bindable(…)` / `$effect.root(…)` / `$effect.tracking()`),
/// peeling transparent author parens. The official candidate gate
/// (`get_rune(init, scope) === null`) EXCLUDES a rune-initialized declaration
/// from the store-base set: `let state = $state(0)` declares a RUNE binding, so
/// a `$state` reference stays a rune, never a subscription over `state`.
/// (Official re-admits a `$props()`-init base as a prop-backed store — a
/// deliberately NOT-implemented surface here; the exclusion keeps that shape
/// fail-closed downstream instead of mis-emitting a plain store read.)
fn declarator_init_is_rune_call(init: &Expression<'_>) -> bool {
    let mut expr = init;
    while let Expression::ParenthesizedExpression(p) = expr {
        expr = &p.expression;
    }
    let Expression::CallExpression(call) = expr else {
        return false;
    };
    let root = match &call.callee {
        Expression::Identifier(id) => id.name.as_str(),
        Expression::StaticMemberExpression(m) => match &m.object {
            Expression::Identifier(id) => id.name.as_str(),
            _ => return false,
        },
        _ => return false,
    };
    RUNE_ROOT_NAMES.contains(&root)
}

/// The declared BASE names a `$name` subscription may legally target: every
/// top-level instance-script declaration name (variable / function / class,
/// destructure patterns included) whose declarator init is NOT a rune call,
/// plus every ADMITTED import local from BOTH script slots — read from the
/// single [`ClassifiedScriptImports`] carrier, never a raw import re-walk.
///
/// Exclusions (each an official-rule mirror):
/// - a `$`-prefixed base (its accessor would land in the `$$` magic namespace);
/// - a RUNE-INITIALIZED declarator (`let state = $state(0)` — the official
///   `get_rune(init) === null` gate; the base stays a rune binding);
/// - the import local `derived` FROM `'svelte/store'` (the official
///   `$derived`-vs-`svelte/store` special case: `import { derived } from
///   'svelte/store'` in the same file as a `$derived(…)` rune is NOT a
///   subscription of one to the other).
pub(super) fn store_base_candidates(
    instance_program: Option<&Program<'_>>,
    script_imports: &ClassifiedScriptImports,
) -> FxHashSet<String> {
    let mut out = FxHashSet::default();
    let mut admit = |name: &str| {
        if !name.starts_with('$') {
            out.insert(name.to_string());
        }
    };
    for slot in [UserImportSlot::Module, UserImportSlot::Instance] {
        for import in script_imports.admitted(slot) {
            for (local, _kind) in import_binding_entries(import) {
                // The official `$derived`-from-`'svelte/store'` exclusion: the
                // LOCAL name `derived` imported from `svelte/store` never backs a
                // `$derived` subscription (aliased locals — `derived as dd` /
                // `writable as derived` — key on the LOCAL, matching official's
                // `instance.scope.get(store_name)` lookup).
                if local == "derived" && import.source == "svelte/store" {
                    continue;
                }
                admit(local);
            }
        }
    }
    if let Some(program) = instance_program {
        for stmt in &program.body {
            match stmt {
                Statement::VariableDeclaration(decl) => {
                    for d in &decl.declarations {
                        if d.init.as_ref().is_some_and(declarator_init_is_rune_call) {
                            continue;
                        }
                        let mut names = Vec::new();
                        collect_pattern_names(&d.id, &mut names);
                        for name in names {
                            admit(&name);
                        }
                    }
                }
                Statement::FunctionDeclaration(func) => {
                    if let Some(id) = &func.id {
                        admit(id.name.as_str());
                    }
                }
                Statement::ClassDeclaration(class) => {
                    if let Some(id) = &class.id {
                        admit(id.name.as_str());
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The RUNE-ROOT accessor names EXEMPTED from rune classification because
/// their base is a declared store candidate (`state` ∈ candidates ⇒ `$state`
/// is a store ACCESSOR reference, not a rune). The rune scans (the
/// unsupported-rune position scan, the legacy rune-reference gate, the
/// runes-mode detector) consult this set so a store named a rune-root word is
/// never refused — or mode-flipped — as a rune (official deletes
/// store-classified names from the reference set BEFORE `some(is_rune)`).
pub(super) fn rune_root_accessor_exemptions(candidates: &FxHashSet<String>) -> FxHashSet<String> {
    RUNE_ROOT_NAMES
        .iter()
        .filter(|rune| candidates.contains(&rune[1..]))
        .map(|rune| (*rune).to_string())
        .collect()
}

/// Declare one [`BindingRuntimeKind::StoreSubscription`] binding per candidate
/// base (`count` → the `$count` accessor binding) in the instance ROOT scope,
/// so a `$count` read/write resolves scope-awarely wherever the shared binding
/// table is consulted (interpolation classification, the expression rewriter,
/// bind-target classification). An unreferenced accessor binding is inert.
pub(super) fn prepare_store_subscription_bindings(
    candidates: &FxHashSet<String>,
    scope: ScopeId,
    scopes: &mut ScopeGraph,
    bindings: &mut BindingTable,
) {
    // Deterministic declaration order (the candidate set is a hash set).
    let mut ordered: Vec<&String> = candidates.iter().collect();
    ordered.sort();
    for base in ordered {
        let name = format!("${base}");
        let binding = bindings.push(BindingInfo {
            name: name.clone(),
            scope,
            kind: BindingRuntimeKind::StoreSubscription,
            state: None,
        });
        scopes.declare(scope, &name, binding);
    }
}

/// The classify-time `$store` fact bundle — the ordered subscriptions, the
/// demand-driven const/function admission closure, the top-level function
/// referent set, and the rune-root accessor exemption set — computed ONCE per
/// component by the default-deny classifier and consumed by the item
/// classifier, the event-handler classification, the legacy gate, the rune
/// scan, and the plan.
pub(super) struct ClassifiedStoreFacts {
    /// The classified subscriptions in first-seen order.
    pub(super) subscriptions: Vec<StoreSubscription>,
    /// The top-level `const` names the dependency closure admits.
    pub(super) const_names: FxHashSet<String>,
    /// The top-level `class` names the dependency closure admits (a local
    /// store CLASS reached transitively from a subscribed base — `const c = new
    /// S()` admits `S`).
    pub(super) class_names: FxHashSet<String>,
    /// The top-level `function` names the dependency closure admits.
    pub(super) closure_fns: FxHashSet<String>,
    /// The top-level named `function` declaration names (the bare-identifier
    /// event-handler referent set).
    pub(super) fn_decl_names: FxHashSet<String>,
    /// The rune-root ACCESSOR names whose base is a store candidate (`$state`
    /// when `state` is a declared non-rune-init base) — the exemption set the
    /// rune scans consult so a store accessor is never classified as a rune.
    pub(super) rune_exempt_accessors: FxHashSet<String>,
}

/// Compute the whole classify-time `$store` fact bundle for one component: the
/// candidate bases (declarations + admitted import locals), the ordered
/// scope-aware subscription scan over the instance program + every analyzed
/// template expression, the demand-driven admission closure the subscriptions
/// seed, and the top-level function referents. FALLIBLE: a `$NAME` whose base
/// resolves to a NON-top-level binding is the official
/// `store_invalid_scoped_subscription` reject and fails the component closed.
pub(super) fn classify_store_facts(
    ir: &super::ir::SvelteRuntimeIr,
) -> Result<ClassifiedStoreFacts, UnsupportedSvelteRuntimeSurface> {
    let instance_source = ir.analysis.scripts.instance_source;
    let instance_program = ir.analysis.scripts.instance_program.as_ref();
    let candidates = store_base_candidates(instance_program, &ir.analysis.script_imports);
    let rune_exempt_accessors = rune_root_accessor_exemptions(&candidates);
    let root_scope = ir.root_scope().scope;
    let module_scope = ir.analysis.scopes.parent(root_scope);
    let subscriptions = collect_store_subscriptions(
        instance_program,
        ir.analysis.expressions.all(),
        &ir.analysis.scopes,
        &ir.analysis.bindings,
        root_scope,
        module_scope,
        &candidates,
    )?;
    let subscribed_bases: FxHashSet<String> =
        subscriptions.iter().map(|s| s.base().to_string()).collect();
    let (const_names, closure_fns, class_names) =
        store_dependency_closure(instance_source, instance_program, &subscribed_bases);
    let fn_decl_names = collect_top_level_function_names(instance_program);
    Ok(ClassifiedStoreFacts {
        subscriptions,
        const_names,
        class_names,
        closure_fns,
        fn_decl_names,
        rune_exempt_accessors,
    })
}

/// Fail closed on a RUNE-USAGE × STORE-SUBSCRIPTION collision. When a
/// rune-root-NAMED accessor is a classified subscription (`$state` over `const
/// state = writable(0)`) AND the SAME rune root has live admitted usage in the
/// component (a `let n = $state(1)` StatePrimitive item, an effect
/// statement/init, an elided `$inspect`, a `$props()` destructure or
/// `$props.id()` decl, an admitted `$host()` call), official treats EVERY
/// reference of that name as the STORE ACCESSOR (first-hand probe: `let n =
/// $state(1)` compiles to `let n = $state()(1);` under LEGACY mode) — a lowering
/// this backend does not implement (it needs the legacy-let vertical). Emitting
/// would ship a DIVERGENT-mode module, so the collision refuses with a precise
/// instance-script-item diagnostic. `Ok(())` when no subscription collides.
pub(super) fn detect_rune_usage_store_collision(
    script_items: &[super::instance_items::SupportedInstanceScriptItem],
    subscriptions: &[StoreSubscription],
    uses_host: bool,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    use super::instance_items::SupportedInstanceScriptItem as Item;
    if subscriptions.is_empty() {
        return Ok(());
    }
    let mut used_rune_roots: FxHashSet<&'static str> = FxHashSet::default();
    for item in script_items {
        match item {
            Item::StatePrimitive { .. } => {
                used_rune_roots.insert("$state");
            }
            Item::EffectStatement { .. } | Item::EffectRuneInit { .. } => {
                used_rune_roots.insert("$effect");
            }
            Item::InspectElided => {
                used_rune_roots.insert("$inspect");
            }
            Item::PropsDestructure | Item::PropsIdDecl { .. } => {
                used_rune_roots.insert("$props");
                used_rune_roots.insert("$bindable");
            }
            _ => {}
        }
    }
    if uses_host {
        used_rune_roots.insert("$host");
    }
    for sub in subscriptions {
        if used_rune_roots.contains(sub.name.as_str()) {
            return Err(UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
                construct: "rune-named store accessor colliding with rune usage",
                span: Span::new(0, 0),
            });
        }
    }
    Ok(())
}

/// The scope-aware, first-seen-ordered `$store` subscription scan.
///
/// Walks the instance program first (a lexical frame stack over the real
/// declared names — function/arrow params, nested `let`/`const`/`var`, `catch`
/// params, `for` bindings), then every ANALYZED template expression in arena
/// (document) order through its real [`AnalyzedExpr::scope`] + the shared
/// [`ScopeGraph`] — mirroring the official analyze walk order the accessor
/// emission is pinned to (`{$b} {$a}` mints `$b` before `$a`). NO expression
/// source is reparsed.
///
/// Per subscription-shaped `$NAME` reference ([`is_subscription_shaped`]):
/// - a local of the same literal `$name` (the `derived(a, ($a) => …)` callback
///   param) owns the reference — a plain local read, never a subscription;
/// - the BASE resolving to a NON-top-level binding — a script function
///   parameter / nested local, a template `{#each as x}` alias, a `{#snippet}`
///   param, an `{#await then x}` binding, an expression-local arrow param
///   ([`super::expr::ExprReference::store_base_locally_bound`]) — is the
///   official `store_invalid_scoped_subscription` COMPILE ERROR → `Err`;
/// - the base resolving to a TOP-LEVEL candidate (root/module scope, or an
///   unregistered top-level declaration in the candidate set) → a
///   subscription;
/// - anything else → not a store (the rune / unresolved-name gates own it).
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_store_subscriptions(
    instance_program: Option<&Program<'_>>,
    template_exprs: &[AnalyzedExpr<'_>],
    scopes: &ScopeGraph,
    bindings: &BindingTable,
    root_scope: ScopeId,
    module_scope: Option<ScopeId>,
    candidates: &FxHashSet<String>,
) -> Result<Vec<StoreSubscription>, UnsupportedSvelteRuntimeSurface> {
    let mut scan = ScriptSubscriptionScan {
        candidates,
        frames: Vec::new(),
        seen: FxHashSet::default(),
        ordered: Vec::new(),
        scoped_reject: None,
    };
    if let Some(program) = instance_program {
        scan.visit_program(program);
    }
    if let Some((name, span)) = scan.scoped_reject {
        return Err(UnsupportedSvelteRuntimeSurface::StoreScopedSubscription { name, span });
    }
    let ScriptSubscriptionScan {
        mut seen,
        mut ordered,
        ..
    } = scan;
    for expr in template_exprs {
        for reference in &expr.references {
            let name = reference.name.as_str();
            if !is_subscription_shaped(name) {
                continue;
            }
            // An expression-INTERNAL base shadow (`onclick={(x) => $x}`): the
            // arrow param owns the base where `$x` is read — official rejects.
            if reference.store_base_locally_bound {
                return Err(UnsupportedSvelteRuntimeSurface::StoreScopedSubscription {
                    name: name.to_string(),
                    span: Span::new(0, 0),
                });
            }
            let base = &name[1..];
            // A resolved base owned by a TEMPLATE-BLOCK / non-top-level scope (an
            // each alias, a snippet param, an await binding, a slot `let:` local)
            // — official rejects the scoped subscription. An UNREGISTERED base (a
            // top-level `const`/`function`/`class` — the plain-local pass
            // registers `let` locals only) is top-level by construction; the
            // candidate check below decides.
            if let Some(id) = scopes.resolve(bindings, expr.scope, base) {
                let owner = bindings.get(id).scope;
                if owner != root_scope && Some(owner) != module_scope {
                    return Err(UnsupportedSvelteRuntimeSurface::StoreScopedSubscription {
                        name: name.to_string(),
                        span: Span::new(0, 0),
                    });
                }
            }
            if candidates.contains(base) && seen.insert(name.to_string()) {
                ordered.push(StoreSubscription {
                    name: name.to_string(),
                });
            }
        }
    }
    Ok(ordered)
}

/// The demand-driven top-level admission closure, seeded by the SUBSCRIBED
/// bases: a subscribed name admits its own top-level `const` / `function` /
/// `class` declaration, and each admitted declaration's init / body free
/// references admit the sibling top-level `const`s / `function`s / `class`es
/// they name, transitively (`const doubled = derived(a, …)` admits `a`; `const
/// c = w(0)` admits the factory `w`; `const c = new S()` admits the store class
/// `S`). Returns `(admitted const names, admitted function names, admitted class
/// names)`. With NO subscription the closure is empty — an arbitrary
/// call-initialized `const x = make()` stays refused, and a `class` not reached
/// from any subscription stays fail-closed (out of the store-subscription scope).
pub(super) fn store_dependency_closure(
    instance_source: Option<&str>,
    instance_program: Option<&Program<'_>>,
    subscribed_bases: &FxHashSet<String>,
) -> (FxHashSet<String>, FxHashSet<String>, FxHashSet<String>) {
    let mut admitted_consts = FxHashSet::default();
    let mut admitted_fns = FxHashSet::default();
    let mut admitted_classes = FxHashSet::default();
    let (Some(instance), Some(program)) = (instance_source, instance_program) else {
        return (admitted_consts, admitted_fns, admitted_classes);
    };
    // The top-level closure candidates: single-declarator identifier `const`s
    // (name → init source), named `function` declarations (name → source), and
    // named `class` declarations (name → source). A class is admitted the SAME
    // demand-driven way const/function are — reached transitively from a
    // subscribed base (`const c = new S()` names `S`) — and lowered verbatim.
    let mut const_inits: FxHashMap<String, &str> = FxHashMap::default();
    let mut fn_sources: FxHashMap<String, &str> = FxHashMap::default();
    let mut class_sources: FxHashMap<String, &str> = FxHashMap::default();
    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(decl) if decl.kind == VariableDeclarationKind::Const => {
                if let [d] = decl.declarations.as_slice() {
                    if let BindingPattern::BindingIdentifier(id) = &d.id {
                        if let Some(init) = &d.init {
                            use oxc_span::GetSpan;
                            let span = init.span();
                            if let Some(src) = instance.get(span.start as usize..span.end as usize)
                            {
                                const_inits.insert(id.name.to_string(), src);
                            }
                        }
                    }
                }
            }
            Statement::FunctionDeclaration(func) => {
                if let Some(id) = &func.id {
                    let span = func.span;
                    if let Some(src) = instance.get(span.start as usize..span.end as usize) {
                        fn_sources.insert(id.name.to_string(), src);
                    }
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    let span = class.span;
                    if let Some(src) = instance.get(span.start as usize..span.end as usize) {
                        class_sources.insert(id.name.to_string(), src);
                    }
                }
            }
            _ => {}
        }
    }
    let mut worklist: Vec<String> = subscribed_bases.iter().cloned().collect();
    worklist.sort();
    let mut visited: FxHashSet<String> = FxHashSet::default();
    while let Some(name) = worklist.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let referenced_source = if let Some(init) = const_inits.get(name.as_str()) {
            admitted_consts.insert(name.clone());
            Some(*init)
        } else if let Some(source) = fn_sources.get(name.as_str()) {
            admitted_fns.insert(name.clone());
            Some(*source)
        } else if let Some(source) = class_sources.get(name.as_str()) {
            admitted_classes.insert(name.clone());
            Some(*source)
        } else {
            None
        };
        if let Some(src) = referenced_source {
            if let Ok(facts) = super::expr::collect_expr_references(src) {
                for reference in &facts.references {
                    if !visited.contains(&reference.name) {
                        worklist.push(reference.name.clone());
                    }
                }
            }
        }
    }
    (admitted_consts, admitted_fns, admitted_classes)
}

/// The top-level named `function` declaration names of the instance script —
/// the referent set a bare-identifier event handler (`onclick={inc}`) may
/// legally name.
pub(super) fn collect_top_level_function_names(
    instance_program: Option<&Program<'_>>,
) -> FxHashSet<String> {
    let mut out = FxHashSet::default();
    let Some(program) = instance_program else {
        return out;
    };
    for stmt in &program.body {
        if let Statement::FunctionDeclaration(func) = stmt {
            if let Some(id) = &func.id {
                out.insert(id.name.to_string());
            }
        }
    }
    out
}

/// The scope-aware INSTANCE-PROGRAM subscription scan: a lexical frame stack
/// over the real declared names (frame 0 = the program's own top level; every
/// nested frame is a function/arrow/block/catch/for scope). Per
/// subscription-shaped `$NAME` reference:
/// - the literal `$name` bound by ANY frame → a plain local read (skip);
/// - the BASE bound by an INNER (non-program) frame → the official
///   `store_invalid_scoped_subscription` reject (recorded, first wins);
/// - the base a declared candidate → a subscription (first-seen ordered).
struct ScriptSubscriptionScan<'a> {
    candidates: &'a FxHashSet<String>,
    /// The lexical frames; `frames[0]` is the PROGRAM (top-level) frame.
    frames: Vec<FxHashSet<String>>,
    seen: FxHashSet<String>,
    ordered: Vec<StoreSubscription>,
    /// The FIRST scoped-subscription violation (accessor name + span).
    scoped_reject: Option<(String, Span)>,
}

/// Whether the class declaration body carries ANY inner `$`-store/rune reactive
/// reference — a `$NAME` store read/write or a rune reference (`$state` /
/// `$derived` / …), excluding the `$$`-magic namespace. Walks the OXC class
/// subtree (method / getter / setter bodies, field/property initializers, static
/// blocks, and computed member keys) for any `IdentifierReference` whose name is
/// [`is_subscription_shaped`]. Verbatim class-body lowering
/// ([`super::instance_items::SupportedInstanceScriptItem::StoreClassDecl`]) cannot
/// rewrite such a reference the way official `svelte@5.56.3` does (an inner `$a`
/// read → `$a()`, an inner `$a = v` write → `$.store_set(a, v)`), so a class
/// carrying one fails closed. A `$`-named METHOD / PROPERTY KEY is an
/// `IdentifierName`, NOT an `IdentifierReference`, so it is correctly not tripped.
pub(super) fn class_body_has_inner_reactive_reference(class: &oxc_ast::ast::Class<'_>) -> bool {
    struct InnerReactiveScan {
        found: bool,
    }
    impl<'a> Visit<'a> for InnerReactiveScan {
        fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
            if is_subscription_shaped(it.name.as_str()) {
                self.found = true;
            }
            walk::walk_identifier_reference(self, it);
        }
    }
    let mut scan = InnerReactiveScan { found: false };
    walk::walk_class(&mut scan, class);
    scan.found
}

impl ScriptSubscriptionScan<'_> {
    /// Whether the literal `name` is bound by any active frame.
    fn literal_bound(&self, name: &str) -> bool {
        self.frames.iter().any(|f| f.contains(name))
    }

    /// Whether `base` is bound by an INNER (non-program) frame — the
    /// non-top-level ownership that makes a `$base` subscription the official
    /// scoped-subscription reject.
    fn base_inner_bound(&self, base: &str) -> bool {
        self.frames.iter().skip(1).any(|f| f.contains(base))
    }

    /// Observe one identifier reference.
    fn observe(&mut self, name: &str, span: Span) {
        if self.scoped_reject.is_some() || !is_subscription_shaped(name) {
            return;
        }
        if self.literal_bound(name) {
            return;
        }
        let base = &name[1..];
        if self.base_inner_bound(base) {
            self.scoped_reject = Some((name.to_string(), span));
            return;
        }
        if self.candidates.contains(base) && self.seen.insert(name.to_string()) {
            self.ordered.push(StoreSubscription {
                name: name.to_string(),
            });
        }
    }
}

impl<'a> Visit<'a> for ScriptSubscriptionScan<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        let mut frame = FxHashSet::default();
        collect_direct_decls(&it.body, &mut frame);
        collect_var_hoists(&it.body, &mut frame);
        self.frames.push(frame);
        walk::walk_program(self, it);
        self.frames.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.frames.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.frames.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.frames.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.frames.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.frames.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.frames.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        self.frames.push(frame);
        walk::walk_catch_clause(self, it);
        self.frames.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.frames.push(frame);
        walk::walk_for_statement(self, it);
        self.frames.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.frames.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.frames.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.frames.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.frames.pop();
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.observe(it.name.as_str(), Span::new(it.span.start, it.span.end));
        walk::walk_identifier_reference(self, it);
    }
}

#[cfg(test)]
#[path = "store_subscriptions_tests.rs"]
mod tests;
