//! The Svelte client INSTANCE-SCRIPT declaration lowering + `$props()` / `$state`
//! shape classification.
//!
//! This module owns the SCRIPT-side emission: lowering the instance script's
//! top-level rune declarations (`$state` / `$derived` / `$effect` / basic
//! `$props()`) into their emitted client forms, classifying the `$props()` shape
//! (basic vs advanced) and the `$state` declarator shape, collecting the per-name
//! `$props()` read forms, and detecting whether the script uses `$effect`. The
//! per-expression rewriting it drives (every source-derived payload routes through
//! it) is the FALLIBLE two-pass rewriter in [`super::expr_rewrite`].

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, CallExpression, Expression, Statement};

use super::expr::{
    is_bindable_call, is_effect_callee, is_props_callee, reparse_module, state_rune_call,
    BindingTable, StateLowering,
};
use super::expr_rewrite::{PropRead, PropReads};

/// The structural shape of a `$state` / `$state.raw` declarator — the fail-closed
/// gate that distinguishes a SUPPORTED primitive-literal plain-identifier state
/// declarator (`let c = $state(0)`) from the ADVANCED forms the Svelte client
/// emitter refuses (5g) rather than partially lowering: a destructured one
/// (`let { a } = $state(...)` / `let [x] = $state(...)`), OR a NON-primitive-literal
/// initializer (an object / array / call / identifier — `let o = $state({})` /
/// `$state([])` — which official lowers via the `$.proxy` deep-reactive form). Only
/// a string / number / boolean / null / undefined / bigint LITERAL init is the
/// §1.2-class primitive `$.state(<literal>)` form; object/array proxy state is a
/// deferral-ledger item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateDeclShape {
    /// No `$state` declarator (the common case).
    None,
    /// A primitive-literal plain-identifier `$state` declarator (`let c = $state(0)`)
    /// — supported.
    Identifier,
    /// A destructured `$state` declarator, or a non-primitive-literal init (an
    /// object / array / call / identifier argument) — ADVANCED (5g), fails closed.
    Advanced {
        /// A short label for the diagnostic.
        rune: &'static str,
    },
}

/// Classify EVERY `$state` / `$state.raw` declarator's shape in an instance-script
/// source. Returns [`StateDeclShape::Advanced`] if ANY `$state` declarator (not just
/// the first) has a non-plain-identifier pattern (an object/array destructure) OR a
/// NON-primitive-literal initializer (an object / array / call / identifier),
/// [`StateDeclShape::Identifier`] when at least one `$state` declarator exists and
/// ALL are primitive-literal plain identifiers, and [`StateDeclShape::None`] when
/// there is no `$state` declarator. Drives the fail-closed gate BEFORE lowering, so
/// a destructured / object / array `$state` never reaches `lower_state_declarator`.
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
    // Whether the instance script DECLARES a local named `undefined` (a `let undefined
    // = …`). The `$state(undefined)` primitive-literal classification is valid ONLY
    // when `undefined` is the global void-0 reference; a SHADOWED `undefined` is a
    // non-literal reference init (official reads the shadow — `$.state($.proxy($.get(
    // undefined)))` for a signal shadow) which is breadth, so it must fail closed.
    let undefined_shadowed = top_level_declares_undefined(&program);
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
            // (5g) rather than silently dropping the extra argument (`$state(0, 1)` →
            // `$.state(0)`). The check precedes the destructure / init-shape gates so an
            // over-arity call fails on arity regardless of its pattern.
            if call.arguments.len() > 1 {
                return StateDeclShape::Advanced {
                    rune: "$state() invalid arguments",
                };
            }
            // An object / array destructure of `$state(...)` is the advanced form
            // (5g) — return immediately so a later identifier declarator cannot mask
            // it. The identifier-only lowering never sees a destructure pattern.
            if !matches!(&d.id, BindingPattern::BindingIdentifier(_)) {
                return StateDeclShape::Advanced {
                    rune: "$state() destructure",
                };
            }
            // A NON-primitive-literal `$state` initializer (an object / array / call /
            // identifier — `$state({})` / `$state([])` / `$state(makeIt())`) is the
            // deep-reactive `$.proxy` form (a BareProxy / StateProxy). Only a
            // primitive literal is the §1.2-class `$.state(<literal>)` signal — fail
            // closed for everything else (5g).
            // TODO(follow-up): lower object/array `$state` to the deep-reactive
            // `$.state($.proxy(init))` BareProxy / StateProxy form (with the proxied
            // member read/write rewrite) instead of failing closed. Owned by the
            // runes-completion block (5g).
            if !state_init_is_primitive_literal(call, undefined_shadowed) {
                return StateDeclShape::Advanced {
                    rune: "$state() non-primitive init",
                };
            }
        }
    }
    if saw_state {
        StateDeclShape::Identifier
    } else {
        StateDeclShape::None
    }
}

/// Whether a `$state(...)` call's argument is a PRIMITIVE LITERAL — a string /
/// number / boolean / null / bigint literal, or an empty `$state()` (the
/// `undefined` form). A `-1` (a `UnaryExpression` over a numeric literal) counts as
/// a primitive literal init. An object / array / call / identifier / template /
/// member argument is NOT a primitive literal (it is the deep-reactive proxy form).
///
/// `undefined_shadowed` is whether a top-level local named `undefined` is declared:
/// when so, a bare `undefined` argument is a SHADOW REFERENCE, NOT the void-0 literal,
/// so it is not a primitive literal (official reads the shadow).
fn state_init_is_primitive_literal(call: &CallExpression<'_>, undefined_shadowed: bool) -> bool {
    let Some(arg) = call.arguments.first() else {
        // `$state()` — the `undefined` init, a primitive.
        return true;
    };
    let Some(expr) = arg.as_expression() else {
        // A spread argument — not a primitive literal.
        return false;
    };
    expr_is_primitive_literal(expr, undefined_shadowed)
}

/// Whether an expression is a primitive literal — a string / number / boolean /
/// null / bigint literal, or a unary `+` / `-` over a numeric / bigint literal
/// (`-1`). A template literal, an object / array / call / identifier / member is
/// NOT a primitive literal.
///
/// A bare `undefined` identifier is the void-0 primitive ONLY when it is NOT shadowed
/// by a local binding (`undefined_shadowed`); a shadowed `undefined` is an ordinary
/// reference (the deep-reactive non-literal form). `NaN` / `Infinity` are NOT treated
/// as primitive literals (official wraps them in `$.proxy(…)`), so they never reach
/// the literal arm.
fn expr_is_primitive_literal(expr: &Expression<'_>, undefined_shadowed: bool) -> bool {
    match expr {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        // `undefined` is a bare identifier reference: the void-0 primitive ONLY when
        // unshadowed. A shadowed `undefined` (`let undefined = …`) is a real reference
        // — official reads the shadow — so it is NOT a primitive literal.
        Expression::Identifier(id) => id.name.as_str() == "undefined" && !undefined_shadowed,
        // A unary `-1` / `+1` over a numeric/bigint literal.
        Expression::UnaryExpression(u) => matches!(
            &u.argument,
            Expression::NumericLiteral(_) | Expression::BigIntLiteral(_)
        ),
        _ => false,
    }
}

/// Whether the program's TOP-LEVEL declarators include a binding named `undefined`
/// (a `let undefined = …` / `const undefined = …` / `var undefined`). Used to detect
/// a SHADOWED `undefined` so a `$state(undefined)` over the shadow is not mistaken for
/// the void-0 primitive literal.
fn top_level_declares_undefined(program: &oxc_ast::ast::Program<'_>) -> bool {
    program.body.iter().any(|stmt| {
        let Statement::VariableDeclaration(decl) = stmt else {
            return false;
        };
        decl.declarations.iter().any(|d| {
            let mut names = Vec::new();
            super::expr::collect_pattern_names(&d.id, &mut names);
            names.iter().any(|n| n == "undefined")
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

/// Whether the instance script uses `$effect(...)` at the top level (drives
/// `$.push`/`$.pop`). A SHADOWED `$effect` (a local of that name) does not count —
/// but a top-level `$effect()` call statement is the supported form.
#[must_use]
pub fn script_uses_effect(alloc: &Allocator, instance_source: &str) -> bool {
    let Some(program) = reparse_module(alloc, instance_source) else {
        return false;
    };
    program.body.iter().any(|stmt| {
        if let Statement::ExpressionStatement(es) = stmt {
            if let Expression::CallExpression(call) = &es.expression {
                return is_effect_callee(&call.callee);
            }
        }
        false
    })
}

/// The lowering of ONE supported instance-script item that needs NO expression
/// rewriter — the simple declaration variants. Returns the emitted client-body
/// statement, [`SimpleItemLowering::None`] for a variant that emits nothing (a
/// no-default `$props()` destructure), or [`SimpleItemLowering::NeedsRewriter`] for the
/// [`FunctionDecl`](super::instance_items::SupportedInstanceScriptItem::FunctionDecl)
/// variant (whose body lowers through the FALLIBLE expression rewriter, owned by the
/// caller that holds the rewriter — [`SupportedClientIr::build_script_items`](super::client_plan::SupportedClientIr)).
///
/// The simple variants are a thin per-variant transform:
/// - [`StatePrimitive`](super::instance_items::SupportedInstanceScriptItem::StatePrimitive)
///   → `let name = $.state(<init>);` (signal) / `let name = <init>;` (never-reassigned
///   `PlainLet`) — the wrapper choice reads the binding's resolved `StateLowering`;
/// - [`PropsDestructure`](super::instance_items::SupportedInstanceScriptItem::PropsDestructure)
///   → NOTHING (a no-default props destructure reads off `$$props`, emitting no decl);
/// - [`BindThisLocal`](super::instance_items::SupportedInstanceScriptItem::BindThisLocal)
///   → `let name;` (the `bind:this` clone-root local);
/// - [`BindLocalLet`](super::instance_items::SupportedInstanceScriptItem::BindLocalLet)
///   → `let name = <init>;` / `let name;` (a plain-local DOM bind-target root, verbatim
///   literal init, or the uninitialized no-init form).
///
/// A primitive-literal init carries no signal read and no TS syntax, so it is emitted
/// verbatim (the over-arity / non-primitive / destructured / `$state.raw` forms were
/// refused upstream).
#[must_use]
pub(super) fn lower_simple_instance_item(
    item: &super::instance_items::SupportedInstanceScriptItem,
    bindings: &BindingTable,
) -> SimpleItemLowering {
    use super::instance_items::SupportedInstanceScriptItem as Item;
    match item {
        Item::StatePrimitive { name, init } => SimpleItemLowering::Statement(
            lower_state_primitive_item(name, init.as_deref(), bindings),
        ),
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
    }
}

/// The outcome of lowering one simple (rewriter-free) instance-script item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SimpleItemLowering {
    /// The emitted client-body statement.
    Statement(String),
    /// The item emits no component-body declaration (a no-default props destructure).
    None,
    /// The item is a `FunctionDecl` whose body lowers through the FALLIBLE expression
    /// rewriter — the caller (which holds the rewriter) handles it.
    NeedsRewriter,
}

/// Lower a supported `$state(<primitive literal>)` item to its emitted declaration.
///
/// The wrapper choice comes from the binding's resolved write-gated
/// [`StateLowering`] (a never-reassigned signal lowers to `PlainLet`, a reassigned
/// one to `StateSignal`), so the emission matches official. A no-arg `$state()` is
/// the SHADOW-ROBUST `void 0` form (never the bare identifier `undefined`); an
/// explicit primitive init is emitted verbatim.
fn lower_state_primitive_item(name: &str, init: Option<&str>, bindings: &BindingTable) -> String {
    let arg = init.unwrap_or("void 0");
    let lowering = bindings
        .all()
        .iter()
        .find(|b| b.name.as_str() == name)
        .and_then(|b| b.state.map(|s| s.lowering));
    match lowering {
        Some(StateLowering::PlainLet) => format!("let {name} = {arg};"),
        Some(StateLowering::StateSignal) | Some(StateLowering::RawStateSignal) => {
            format!("let {name} = $.state({arg});")
        }
        // A primitive `$state` never resolves to a proxy lowering (proxy is the
        // object/array deep-reactive form, refused upstream). An unclassified state
        // (no binding row) is a compiler-invariant violation — emit the signal form
        // (the never-live defensive arm; the allowlist is the authority).
        Some(StateLowering::BareProxy) | Some(StateLowering::StateProxy) | None => {
            format!("let {name} = $.state({arg});")
        }
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
