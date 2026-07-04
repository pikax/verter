//! The Svelte client INSTANCE-SCRIPT declaration lowering + `$props()` / `$state`
//! shape classification.
//!
//! This module owns the SCRIPT-side emission: lowering the instance script's
//! top-level rune declarations (`$state` / `$derived` / `$effect` / basic
//! `$props()`) into their emitted client forms, classifying the `$props()` shape
//! (basic vs advanced) and the `$state` declarator shape, and collecting the
//! per-name `$props()` read forms. The per-expression rewriting it drives (every
//! source-derived payload routes through it) is the FALLIBLE two-pass rewriter in
//! [`super::expr_rewrite`].

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, CallExpression, Expression, Statement};

use super::expr::{
    is_bindable_call, is_props_callee, reparse_module, state_rune_call, BindingTable, StateLowering,
};
use super::expr_rewrite::{PropRead, PropReads};

/// The structural shape of a `$state` / `$state.raw` declarator — the fail-closed
/// gate that distinguishes a SUPPORTED plain-identifier state declarator (a
/// primitive `let c = $state(0)` OR a proxiable object/array/identifier/call
/// `let o = $state({})` — the deep-reactive `$.proxy` form) from the ADVANCED forms
/// the Svelte client emitter refuses rather than partially lowering: a destructured
/// one (`let { a } = $state(...)` / `let [x] = $state(...)`), an over-arity call, a
/// spread argument, or the narrowed form Verter's `should_proxy` predicate mis-decides
/// (a REACTIVE-shadowed bare `undefined` init). A plain-identifier declarator whose init is a
/// primitive OR a proxiable value is SUPPORTED — the declarator emitter lowers it per
/// the resolved [`StateLowering`], routing a proxiable init through the shared
/// expression rewriter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateDeclShape {
    /// No `$state` declarator (the common case).
    None,
    /// A plain-identifier `$state` / `$state.raw` declarator (primitive OR proxiable
    /// init) — supported.
    Identifier,
    /// A destructured `$state` declarator, an over-arity call, a spread argument, or a
    /// reactive-shadowed-`undefined` init — ADVANCED, fails closed.
    Advanced {
        /// A short label for the diagnostic.
        rune: &'static str,
    },
}

/// Classify EVERY `$state` / `$state.raw` declarator's shape in an instance-script
/// source. Returns [`StateDeclShape::Advanced`] if ANY `$state` declarator (not just
/// the first) has a non-plain-identifier pattern (an object/array destructure), an
/// over-arity call, a spread argument, or the narrowed reactive-shadowed-`undefined`
/// init; [`StateDeclShape::Identifier`]
/// when at least one plain-identifier `$state` declarator exists and ALL are supported
/// (a primitive OR proxiable init); and [`StateDeclShape::None`] when there is no
/// `$state` declarator. Drives the fail-closed gate BEFORE lowering, so a
/// destructured `$state` never reaches the declarator emitter.
///
/// Scanning ALL declarators (across all statements AND all declarators within a
/// single multi-declarator statement) is load-bearing: `let ok = $state(0); let {
/// a } = $state({ a: 1 })` must fail closed on the SECOND declarator rather than
/// classify on the first and silently drop the destructured one (which would leave
/// `a` undefined → a runtime `ReferenceError`).
#[must_use]
pub fn state_decl_shape(instance_source: &str) -> StateDeclShape {
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance_source) else {
        return StateDeclShape::None;
    };
    // Whether the instance script shadows `undefined` with a REACTIVE-RUNE binding (a
    // top-level `let undefined = $state(…)` / `$state.raw(…)` / `$derived(…)` /
    // `$derived.by(…)`). This gate is PRE-LOWERING and INSTANCE-ONLY, so it
    // CONSERVATIVELY fails closed over the WHOLE reactive-rune-`undefined`-shadow class —
    // see `state_init_is_shadowed_undefined` for the honest rationale (the LIVE-signal
    // subcase is unsupportable and this gate cannot tell demoted from live here). A
    // PLAIN-local `undefined` shadow (`let undefined = 5`) is NOT reactive: it reads plain
    // and lowers to `$.state(undefined)` matching official — supported.
    let undefined_reactive_shadow = top_level_undefined_shadow_is_reactive_rune(&program);
    let mut saw_state = false;
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let Some(Expression::CallExpression(call)) = &d.init else {
                continue;
            };
            if state_rune_call(call).is_none() {
                continue;
            }
            saw_state = true;
            // `$state` / `$state.raw` accept ZERO or ONE argument; a SECOND argument is
            // the official `rune_invalid_arguments_length` compile error ("$state must
            // be called with zero or one arguments", CallExpression.js). Fail closed
            // rather than silently dropping the extra argument (`$state(0, 1)` →
            // `$.state(0)`). The check precedes the destructure / init-shape gates so an
            // over-arity call fails on arity regardless of its pattern.
            if call.arguments.len() > 1 {
                return StateDeclShape::Advanced {
                    rune: "$state() invalid arguments",
                };
            }
            // A SPREAD argument (`$state(...x)` / `$state.raw(...x)`) is the official
            // `rune_invalid_spread` compile error ("`$state` cannot be called with a
            // spread argument"). A single spread arg is `arguments.len() == 1` and its
            // `as_expression()` is `None`, so it slips past BOTH the arity gate above
            // and the init-shape gates below (`state_primitive_init_text` would emit
            // `void 0` for the missing expression) — fail closed instead. Checked over
            // EVERY argument (`.any(as_expression().is_none())`) so no non-expression
            // argument form reaches the `void 0` init fallback.
            if call.arguments.iter().any(|a| a.as_expression().is_none()) {
                return StateDeclShape::Advanced {
                    rune: "$state() spread argument",
                };
            }
            // An object / array destructure of `$state(...)` is the advanced form — the
            // official compiler lowers it through a destructure closure; that lowering
            // is a distinct surface, so it fails closed (returned immediately so a later
            // identifier declarator cannot mask it).
            if !matches!(&d.id, BindingPattern::BindingIdentifier(_)) {
                return StateDeclShape::Advanced {
                    rune: "$state() destructure",
                };
            }
            // CONSERVATIVE fail-close: a `$state(undefined)` init whose `undefined` is
            // shadowed by a REACTIVE-RUNE binding (`$state` / `$state.raw` / `$derived`).
            // The correct output depends on how that shadow LOWERS (oracle-verified
            // against `svelte@5.56.3`):
            //   - a LIVE-signal shadow (reassigned as a whole variable → lowers to
            //     `$.state(…)`, read via `$.get`) makes the init read `$.get(undefined)`,
            //     which official proxies → `$.state($.proxy($.get(undefined)))`. Verter
            //     CANNOT reproduce this (its `expr_is_proxiable` hardcodes the `undefined`
            //     identifier non-proxiable and would omit the `$.proxy`), so the LIVE
            //     subcase is genuinely UNSUPPORTABLE.
            //   - a DEMOTED shadow (never reassigned → lowers to a plain `let undefined =
            //     …` / `$.proxy(…)`, read BARE) makes the init read a bare `undefined` →
            //     official emits `$.state(undefined)`, which Verter CAN reproduce.
            // This gate is PRE-LOWERING and INSTANCE-ONLY: it cannot know whether the
            // shadow will be demoted or live, because a whole-variable reassignment that
            // promotes the shadow to a live signal can happen in a TEMPLATE handler this
            // gate never sees (`onclick={() => undefined = 1}` — oracle-verified to promote
            // the shadow to `$.state(…)`). Distinguishing demoted-vs-live SAFELY requires
            // the whole-component resolved binding lowering, computed downstream in
            // `state_prep` AFTER template writes are attributed — which inverts this gate's
            // pre-lowering ordering. So Verter CONSERVATIVELY refuses the WHOLE
            // reactive-rune-`undefined`-shadow class (fail-closed-safe). This OVER-REFUSES
            // the demoted subcase; a PLAIN-local shadow (`let undefined = 5`) is NOT
            // reactive and stays SUPPORTED (`$.state(undefined)`).
            // TODO(follow-up): make this gate LOWERING-AWARE — consult the whole-component
            // resolved `StateLowering` of the `undefined` shadow (demoted `PlainLet` /
            // `BareProxy` → support `$.state(undefined)`; live `StateSignal` / `StateProxy`
            // / `RawStateSignal` → fail closed). Requires running the shared write analysis
            // (instance + template) BEFORE this pre-lowering gate; the current over-refusal
            // is fail-closed-safe until then.
            if state_init_is_shadowed_undefined(call, undefined_reactive_shadow) {
                return StateDeclShape::Advanced {
                    rune: "$state() shadowed undefined init",
                };
            }
            // NARROWED fail-close: a TS-WRAPPED init (`$state(0 as number)` /
            // `$state(x satisfies T)` / `$state(x!)` / `$state(<T>x)`) is a distinct
            // surface — official's plain-JS parse REJECTS the cast, and Verter's
            // proxiability predicate would mis-decide the wrapped node (a `0 as number`
            // is `$.state(0)` non-proxiable in official, but the wrapper reads proxiable
            // here). Fail closed rather than mis-lower (a `lang="ts"` widening is a
            // separate surface).
            // TODO(follow-up): strip the TS wrapper spine to its inner expression and
            // run the proxy decision over that, so a `lang="ts"` `$state(0 as number)`
            // lowers to `$.state(0)` instead of failing closed (the `lang="ts"` script
            // widening is the owning surface).
            if state_init_has_ts_wrapper(call) {
                return StateDeclShape::Advanced {
                    rune: "$state() ts-wrapped init",
                };
            }
            // Everything else — a primitive literal OR a proxiable object / array /
            // identifier / call / member / `NaN` / `Infinity` / unshadowed `undefined`
            // init — is SUPPORTED: the declarator emitter lowers it per the resolved
            // `StateLowering`, routing a proxiable init through the shared rewriter so a
            // signal read inside it becomes `$.get`.
        }
    }
    if saw_state {
        StateDeclShape::Identifier
    } else {
        StateDeclShape::None
    }
}

/// Whether a `$state(...)` call's argument is a bare `undefined` identifier that is
/// shadowed by a REACTIVE-RUNE binding (`undefined_reactive_shadow`). Verter
/// CONSERVATIVELY fails this closed — see the call site for the full rationale: the
/// LIVE-signal subcase official emits as `$.state($.proxy($.get(undefined)))` is
/// unsupportable (Verter's `expr_is_proxiable` hardcodes the `undefined` identifier
/// non-proxiable and would omit the `$.proxy`), and this PRE-LOWERING / INSTANCE-ONLY
/// gate cannot distinguish it from the DEMOTED subcase (which official supports as
/// `$.state(undefined)`) without the whole-component resolved binding lowering. A
/// PLAIN-local `undefined` shadow (`let undefined = 5`) reads plain and lowers to
/// `$.state(undefined)` matching official — supported. An unshadowed `undefined` (the
/// void-0 global) and a no-arg `$state()` are likewise supported (`$.state(undefined)` /
/// `$.state(void 0)`).
fn state_init_is_shadowed_undefined(
    call: &CallExpression<'_>,
    undefined_reactive_shadow: bool,
) -> bool {
    let Some(arg) = call.arguments.first() else {
        // `$state()` — the void-0 primitive form, never shadowed-undefined.
        return false;
    };
    let Some(Expression::Identifier(id)) = arg.as_expression() else {
        return false;
    };
    id.name.as_str() == "undefined" && undefined_reactive_shadow
}

/// Whether a `$state(...)` init's argument is a TOP-LEVEL TypeScript wrapper — an
/// `as` / `satisfies` / non-null `!` / type-assertion expression (paren-transparent).
/// Such an init is a distinct surface (official's plain-JS parse rejects the cast, and
/// the proxiability predicate mis-reads the wrapped node), so it fails closed.
fn state_init_has_ts_wrapper(call: &CallExpression<'_>) -> bool {
    let Some(arg) = call.arguments.first() else {
        return false;
    };
    let Some(mut expr) = arg.as_expression() else {
        return false;
    };
    while let Expression::ParenthesizedExpression(p) = expr {
        expr = &p.expression;
    }
    matches!(
        expr,
        Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSTypeAssertion(_)
    )
}

/// Whether the program's TOP-LEVEL declarators shadow `undefined` with a
/// REACTIVE-RUNE binding — a `let undefined = $state(…)` / `$state.raw(…)` /
/// `$derived(…)` / `$derived.by(…)`. Such a shadow MAY lower to a live signal (read via
/// `$.get`), in which case official proxies the init (`$.state($.proxy($.get(undefined)))`)
/// — a form Verter cannot reproduce. This PRE-LOWERING / INSTANCE-ONLY gate cannot tell
/// the live subcase from the DEMOTED subcase (a shadow reassigned only in a template
/// handler still becomes a live signal, and this gate never sees the handler), so Verter
/// CONSERVATIVELY flags the WHOLE class — see `state_init_is_shadowed_undefined` and its
/// call site. A PLAIN-local `undefined` shadow (`let undefined = 5`) is NOT reactive: it
/// reads plain and lowers to `$.state(undefined)` matching official, so it is not flagged.
fn top_level_undefined_shadow_is_reactive_rune(program: &oxc_ast::ast::Program<'_>) -> bool {
    program.body.iter().any(|stmt| {
        let Statement::VariableDeclaration(decl) = stmt else {
            return false;
        };
        decl.declarations.iter().any(|d| {
            // The declarator must BIND `undefined`.
            let mut names = Vec::new();
            super::expr::collect_pattern_names(&d.id, &mut names);
            if !names.iter().any(|n| n == "undefined") {
                return false;
            }
            // …with a REACTIVE-RUNE initializer (`$state` / `$state.raw` / `$derived`
            // / `$derived.by`). A plain / non-rune init is a non-reactive shadow.
            let Some(Expression::CallExpression(call)) = &d.init else {
                return false;
            };
            state_rune_call(call).is_some() || super::expr::is_derived_callee(&call.callee)
        })
    })
}

// ---------------------------------------------------------------------------
// Instance-script declaration lowering
// ---------------------------------------------------------------------------

/// The shape of a component's `$props()` usage — drives the basic-vs-advanced
/// fail-closed decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropsShape {
    /// No `$props()` call.
    None,
    /// A basic destructure: `let { a, b = 1 } = $props()` (named members + native
    /// defaults, no rest / `$bindable` / whole-object).
    BasicDestructure,
    /// An advanced form that fails closed (a rest member, a whole-object
    /// identifier binding, or a `$bindable()` default).
    Advanced {
        /// A short rune label for the diagnostic.
        rune: &'static str,
    },
}

/// Collect the per-name `$props()` read forms from the instance script: a
/// default-bearing member is a getter call (`name()`); a no-default member is a
/// direct props access (`$$props.name`). An empty map when there is no `$props()`.
#[must_use]
pub fn collect_prop_reads(alloc: &Allocator, instance_source: &str) -> PropReads {
    let mut reads = PropReads::default();
    let Some(program) = reparse_module(alloc, instance_source) else {
        return reads;
    };
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let Some(Expression::CallExpression(call)) = &d.init else {
                continue;
            };
            if !is_props_callee(&call.callee) {
                continue;
            }
            if let BindingPattern::ObjectPattern(obj) = &d.id {
                for prop in &obj.properties {
                    // The SOURCE prop key (the destructure key), which may differ
                    // from the local binding name under aliasing
                    // (`let { foo: bar }` → key `foo`, local `bar`).
                    let key = prop_key_name(prop);
                    match &prop.value {
                        // A default-bearing member is declared via `$.prop` (getter)
                        // keyed on the LOCAL name; the source key lives in the decl.
                        BindingPattern::AssignmentPattern(assign) => {
                            let local = single_ident(&assign.left).unwrap_or(&key).to_string();
                            reads.insert(local, PropRead::Getter);
                        }
                        // A no-default member reads off the props object by its
                        // SOURCE key, under the LOCAL binding name (which may be an
                        // alias): `let { foo: bar }` → read `bar` as `$$props.foo`.
                        BindingPattern::BindingIdentifier(id) => {
                            reads.insert(
                                id.name.to_string(),
                                PropRead::PropsMember {
                                    source_key: key.clone(),
                                },
                            );
                        }
                        _ => {
                            reads.insert(key.clone(), PropRead::PropsMember { source_key: key });
                        }
                    }
                }
            }
        }
    }
    reads
}

/// Classify the instance script's `$props()` usage, scanning EVERY `$props()`
/// declarator (across ALL statements AND all declarators within a multi-declarator
/// statement) — not just the first.
///
/// The official compiler supports exactly ONE top-level `$props()` destructure: a
/// second `$props()` call is `props_duplicate`, and any non-basic shape (a computed
/// / numeric / nested key, a rest, a whole-object binding, a `$bindable()` default)
/// is `props_invalid_pattern`. Scanning ALL declarators is load-bearing:
/// `let {a}=$props(), {[k]:b}=$props()` must fail closed on the SECOND
/// (computed-key) declarator rather than classify on the first basic one and
/// silently emit a raw prop read for `b`. The FIRST advanced shape is reported; if
/// every shape is basic but there are 2+ `$props()` calls, the duplicate is
/// reported.
#[must_use]
pub fn props_shape(instance_source: &str) -> PropsShape {
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance_source) else {
        return PropsShape::None;
    };
    let mut count = 0usize;
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let Some(Expression::CallExpression(call)) = &d.init else {
                continue;
            };
            if !is_props_callee(&call.callee) {
                continue;
            }
            count += 1;
            // `$props()` accepts ZERO arguments; ANY argument is the official
            // `rune_invalid_arguments` compile error ("$props cannot be called with
            // arguments", CallExpression.js). Fail closed (5g) rather than emitting the
            // prop reads regardless of the bogus argument. Checked on the FIRST
            // over-arity declarator so a later valid one cannot mask it.
            if !call.arguments.is_empty() {
                return PropsShape::Advanced {
                    rune: "$props() invalid arguments",
                };
            }
            // Return on the FIRST advanced shape so a later basic declarator cannot
            // mask it.
            match classify_props_pattern(&d.id) {
                PropsShape::Advanced { rune } => return PropsShape::Advanced { rune },
                PropsShape::BasicDestructure | PropsShape::None => {}
            }
        }
    }
    match count {
        0 => PropsShape::None,
        1 => PropsShape::BasicDestructure,
        // Two or more `$props()` calls (all basic) — `props_duplicate`. Fail closed
        // rather than emit two conflicting prop-read surfaces.
        _ => PropsShape::Advanced {
            rune: "$props() duplicate",
        },
    }
}

/// Classify a `$props()` declarator pattern.
fn classify_props_pattern(pattern: &BindingPattern<'_>) -> PropsShape {
    match pattern {
        // A whole-object identifier binding (`let p = $props()`) is advanced.
        BindingPattern::BindingIdentifier(_) => PropsShape::Advanced {
            rune: "$props() whole-object",
        },
        BindingPattern::ObjectPattern(obj) => {
            if obj.rest.is_some() {
                return PropsShape::Advanced {
                    rune: "$props() rest",
                };
            }
            for prop in &obj.properties {
                // A COMPUTED key (`{ [k]: a }`) is rejected by official
                // (`props_invalid_pattern`) — fail closed rather than read the
                // wrong key.
                if prop.computed {
                    return PropsShape::Advanced {
                        rune: "$props() computed key",
                    };
                }
                // Only identifier + string-literal keys are supported. A NUMERIC key
                // (`{ 0: zero }`) reads (in official) off `$$props['0']` — a distinct
                // bracket-access lowering that the basic-destructure path does not
                // produce — so it fails closed rather than reading the wrong key.
                if !matches!(
                    &prop.key,
                    oxc_ast::ast::PropertyKey::StaticIdentifier(_)
                        | oxc_ast::ast::PropertyKey::StringLiteral(_)
                ) {
                    return PropsShape::Advanced {
                        rune: "$props() numeric/computed key",
                    };
                }
                // The member VALUE must be a plain identifier with NO default. A
                // default-bearing member (`{ a = 1 }`) — INCLUDING a constant-literal
                // default (official's flag-3 eager `$.prop($$props, key, 3, <literal>)`
                // form) — is the deferral-ledger props-default surface and fails
                // closed (5g). A `$bindable()` default is the bindable-prop form; a
                // nested destructure is rejected by official.
                // TODO(follow-up): lower a `$props()` member DEFAULT — the official
                // flag-3 eager form for a constant literal (`$.prop($$props, key, 3,
                // <literal>)`) and the lazy flag-19 `get_prop_source` thunk form for a
                // non-literal default. Until then ANY default fails closed above.
                match &prop.value {
                    BindingPattern::BindingIdentifier(_) => {}
                    BindingPattern::AssignmentPattern(assign) => {
                        if is_bindable_call(&assign.right) {
                            return PropsShape::Advanced { rune: "$bindable" };
                        }
                        // A no-default-only props surface — ANY default is a deferral
                        // (5g), whether constant-literal or referencing.
                        return PropsShape::Advanced {
                            rune: "$props() default",
                        };
                    }
                    // An object / array nested destructure value is invalid.
                    BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => {
                        return PropsShape::Advanced {
                            rune: "$props() nested destructure",
                        };
                    }
                }
            }
            PropsShape::BasicDestructure
        }
        // An array destructure / top-level default of `$props()` is unusual —
        // treat as advanced (fail closed rather than partially emit).
        BindingPattern::ArrayPattern(_) | BindingPattern::AssignmentPattern(_) => {
            PropsShape::Advanced {
                rune: "$props() destructure",
            }
        }
    }
}

/// The lowering of ONE supported instance-script item that needs NO expression
/// rewriter — the simple declaration variants. Returns the emitted client-body
/// statement, [`SimpleItemLowering::None`] for a variant that emits nothing (a
/// no-default `$props()` destructure), or [`SimpleItemLowering::NeedsRewriter`] for a
/// variant whose payload lowers through the FALLIBLE expression rewriter (the
/// [`StatePrimitive`](super::instance_items::SupportedInstanceScriptItem::StatePrimitive)
/// init and the [`FunctionDecl`](super::instance_items::SupportedInstanceScriptItem::FunctionDecl)
/// body), owned by the caller that holds the rewriter —
/// [`SupportedClientIr::build_script_items`](super::client_plan::SupportedClientIr).
///
/// The simple variants are a thin per-variant transform:
/// - [`StatePrimitive`](super::instance_items::SupportedInstanceScriptItem::StatePrimitive)
///   → `NeedsRewriter`: the init routes through `rewrite_source` (a signal read inside a
///   proxiable object init → `$.get`, TS stripped) before the `StateLowering` wrapper
///   (`$.state` / `$.proxy` / `$.state($.proxy(…))`) is applied;
/// - [`PropsDestructure`](super::instance_items::SupportedInstanceScriptItem::PropsDestructure)
///   → NOTHING (a no-default props destructure reads off `$$props`, emitting no decl);
/// - [`BindThisLocal`](super::instance_items::SupportedInstanceScriptItem::BindThisLocal)
///   → `let name;` (the `bind:this` clone-root local);
/// - [`BindLocalLet`](super::instance_items::SupportedInstanceScriptItem::BindLocalLet)
///   → `let name = <init>;` / `let name;` (a plain-local DOM bind-target root, verbatim
///   literal init, or the uninitialized no-init form);
/// - [`InspectElided`](super::instance_items::SupportedInstanceScriptItem::InspectElided)
///   → NOTHING (a production-elided `$inspect(...)` / `$inspect(...).with(...)`
///   statement).
#[must_use]
pub(super) fn lower_simple_instance_item(
    item: &super::instance_items::SupportedInstanceScriptItem,
) -> SimpleItemLowering {
    use super::instance_items::SupportedInstanceScriptItem as Item;
    match item {
        // A `$state` / `$state.raw` declarator lowers through the FALLIBLE rewriter: its
        // init routes through `rewrite_source` (a signal read inside a proxiable object
        // init becomes `$.get`, TS is stripped) BEFORE the `StateLowering` wrapper is
        // applied — the caller (which holds the rewriter) handles it.
        Item::StatePrimitive { .. } => SimpleItemLowering::NeedsRewriter,
        // A no-default `$props()` destructure emits no component-body declaration
        // (the props are read directly off `$$props`).
        Item::PropsDestructure => SimpleItemLowering::None,
        Item::BindThisLocal { name } => SimpleItemLowering::Statement(format!("let {name};")),
        // A plain-local DOM bind-target root: the declaration stays a verbatim plain
        // `let name = <literal init>;` (official keeps the plain local), or a bare
        // `let name;` for the uninitialized form. The init was restricted to a
        // literal-only value at classification, so it carries no signal read to
        // rewrite — emitted byte-for-byte.
        Item::BindLocalLet { name, init } => SimpleItemLowering::Statement(match init {
            Some(init) => format!("let {name} = {init};"),
            None => format!("let {name};"),
        }),
        // A named function-pair function: its body lowers through the FALLIBLE rewriter,
        // which lives on the projection — the caller handles it.
        Item::FunctionDecl { .. } => SimpleItemLowering::NeedsRewriter,
        // A top-level `$inspect(...);` / `$inspect(...).with(...);` statement is
        // production-ELIDED: it emits NOTHING (no helper, no import, no dev form —
        // official `dev:false` drops the statement, leaving only a cosmetic `;;`
        // residue Verter does not reproduce). The `.with` context-frame fact is
        // owned by the `needs_context` scan, not this lowering.
        Item::InspectElided => SimpleItemLowering::None,
        // A `$effect(fn);` / `$effect.pre(fn);` statement and a `$effect.root(fn)`
        // / `$effect.tracking()` declarator init lower their payload through the
        // FALLIBLE rewriter (the callee → the registered helper, body signal reads
        // → `$.get`, an `await` refuses) — the caller (which holds the rewriter)
        // handles them.
        Item::EffectStatement { .. } | Item::EffectRuneInit { .. } => {
            SimpleItemLowering::NeedsRewriter
        }
    }
}

/// The outcome of lowering one simple (rewriter-free) instance-script item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SimpleItemLowering {
    /// The emitted client-body statement.
    Statement(String),
    /// The item emits no component-body declaration (a no-default props
    /// destructure, a production-elided `$inspect` statement).
    None,
    /// The item is a `StatePrimitive` / `FunctionDecl` / `EffectStatement` /
    /// `EffectRuneInit` whose payload lowers through the FALLIBLE expression
    /// rewriter — the caller (which holds the rewriter) handles it.
    NeedsRewriter,
}

/// Lower a supported `$state` / `$state.raw` instance item to its emitted declaration.
///
/// The wrapper choice comes from the binding's resolved write-gated [`StateLowering`]
/// (`PlainLet` / `StateSignal` / `RawStateSignal` / `BareProxy` / `StateProxy`), so the
/// emission matches official. `init` is the ALREADY-REWRITTEN init text (a signal read
/// inside a proxiable object init became `$.get`, TS stripped) supplied by the caller; a
/// no-arg `$state()` is the SHADOW-ROBUST `void 0` form (never the bare identifier
/// `undefined`).
pub(super) fn lower_state_primitive_item(
    name: &str,
    init: Option<&str>,
    bindings: &BindingTable,
) -> String {
    // The instance-script `$state` is a ROOT-scope binding (first in the table), so a name
    // lookup is unambiguous here. A BLOCK declarator that could SHADOW an instance binding
    // resolves its lowering by BINDING ID and calls `state_primitive_decl` directly.
    let lowering = bindings
        .all()
        .iter()
        .find(|b| b.name.as_str() == name)
        .and_then(|b| b.state.map(|s| s.lowering));
    state_primitive_decl(name, init, lowering)
}

/// The emitted `$state` / `$state.raw` declaration for an already-RESOLVED write-gated
/// [`StateLowering`], matching pinned `svelte@5.56.3`:
///
/// - `PlainLet`       → `let o = <init>;`                     (never reactively read)
/// - `StateSignal`    → `let o = $.state(<init>);`            (primitive, reassigned)
/// - `RawStateSignal` → `let o = $.state(<init>);`            (`$state.raw`, reassigned — NO `$.proxy`)
/// - `BareProxy`      → `let o = $.proxy(<init>);`            (proxiable, never reassigned)
/// - `StateProxy`     → `let o = $.state($.proxy(<init>));`   (proxiable, reassigned)
///
/// `init` is the FINAL init text — a primitive-literal slice for a block declarator, or
/// the FULLY-REWRITTEN init (signal reads → `$.get`, TS stripped) for an instance-script
/// declarator. The caller resolves `lowering` from the binding it OWNS (by binding id for
/// a block declarator, by root-scope name for an instance item), so a SHADOWING same-name
/// binding can never select the wrong wrapper. A no-arg `$state()` is the SHADOW-ROBUST
/// `void 0` form (never the bare `undefined`).
pub(super) fn state_primitive_decl(
    name: &str,
    init: Option<&str>,
    lowering: Option<StateLowering>,
) -> String {
    let arg = init.unwrap_or("void 0");
    match lowering {
        Some(StateLowering::PlainLet) => format!("let {name} = {arg};"),
        // A primitive signal AND a reassigned `$state.raw` are both a bare
        // `$.state(<init>)` (raw NEVER proxies).
        Some(StateLowering::StateSignal) | Some(StateLowering::RawStateSignal) => {
            format!("let {name} = $.state({arg});")
        }
        // A proxiable object/array `$state` never reassigned is a bare `$.proxy(<init>)`
        // (deep-reactive, NOT a signal).
        Some(StateLowering::BareProxy) => format!("let {name} = $.proxy({arg});"),
        // A proxiable object/array `$state` that is reassigned wraps the proxy in a
        // signal box.
        Some(StateLowering::StateProxy) => format!("let {name} = $.state($.proxy({arg}));"),
        // An unclassified state (no binding row) is a compiler-invariant violation —
        // emit the bare signal form (the never-live defensive arm; the classifier is the
        // authority).
        None => format!("let {name} = $.state({arg});"),
    }
}

/// The destructure key name of an object-pattern property.
fn prop_key_name(prop: &oxc_ast::ast::BindingProperty<'_>) -> String {
    match &prop.key {
        oxc_ast::ast::PropertyKey::StaticIdentifier(id) => id.name.to_string(),
        oxc_ast::ast::PropertyKey::StringLiteral(s) => s.value.to_string(),
        _ => single_ident(&prop.value).unwrap_or("").to_string(),
    }
}

/// The single declared identifier name of a binding pattern, or `None` for a
/// destructure.
fn single_ident<'a>(pattern: &'a BindingPattern<'a>) -> Option<&'a str> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "expr_emit_tests.rs"]
mod tests;
