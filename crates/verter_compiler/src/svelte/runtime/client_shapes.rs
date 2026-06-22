//! Narrow sub-shape classifiers for the default-deny client surface.
//!
//! The default-deny classifier ([`super::client_surface`]) accepts a surface
//! FAMILY (a `bind:value`, a delegated event, a `$props()` destructure). These
//! narrow classifiers tighten that to the EXACT supported SUB-SHAPE and produce a
//! typed accepted-shape FACT (`ClientEventHandlerShape`, `ClientBindShape`, …) the
//! downstream plan/emitter consumes — every sub-shape OUTSIDE the supported
//! boundary fails closed BY CONSTRUCTION (the classifier returns the surface's
//! typed [`UnsupportedSvelteRuntimeSurface`], never a generic accept).
//!
//! Each classifier drives its decision from the typed parse (the OXC AST of the
//! expression source / declarator) + the scope-aware binding table — never a raw
//! text scan.

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Expression, Statement, VariableDeclarationKind};

use super::client::UnsupportedSvelteRuntimeSurface;
use super::expr::{
    bind_target_is_ts_wrapped, classify_bind_target, is_derived_callee, is_props_callee,
    reparse_module, state_rune_call, BindTargetKind, BindingRuntimeKind, BindingTable, ScopeGraph,
    ScopeId,
};
use verter_span::Span;

// ---------------------------------------------------------------------------
// Event-handler shape
// ---------------------------------------------------------------------------

/// The accepted shape of a delegated event handler expression.
///
/// The supported boundary is the §1.2-class handler: a non-async INLINE ARROW
/// whose body is exclusively `$state` assignment / update statements (`() => count
/// += 1`, `() => count++`, `() => { a++; b++; }`). Official emits `$.delegated(...)`
/// with the arrow passed through (rewriting the `$state` reads/writes inside).
/// EVERY other handler shape — a function expression, a local-function identifier, a
/// call, an update of a non-`$state`, a bare member, a sequence, a conditional, an
/// imported identifier, a body containing any non-assignment statement (a call, a
/// declaration, an `if`) — needs the official wrapper / `$.derived` hoist or the
/// arbitrary statement-rewrite breadth and is a deferral-ledger follow-up (5d).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientEventHandlerShape {
    /// `onclick={() => …}` — a non-async arrow whose body is `$state`
    /// assignment / update statement(s).
    Arrow,
}

/// Classify a delegated event handler's expression into its accepted
/// [`ClientEventHandlerShape`], or fail closed.
///
/// Accepts ONLY a non-async arrow-function expression whose body is a simple
/// supported `$state` assignment / update — either an expression body that IS a
/// `$state` assignment / update (`() => count += 1`, `() => count++`) or a block
/// body whose statements are ALL `$state` assignment / update expression statements
/// (`() => { a++; b++; }`). An async arrow is the 5j async surface; every other
/// handler shape (a function expression, a local-function identifier, a call, an
/// update of a non-`$state`, a member, a sequence, a conditional, an imported
/// identifier, a body with any non-assignment statement) is the official wrapper /
/// statement-rewrite breadth (5d).
pub(super) fn classify_event_handler_shape(
    handler_source: &str,
    event_type: &str,
    el_span: Span,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> Result<ClientEventHandlerShape, UnsupportedSvelteRuntimeSurface> {
    let refuse = || UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
        event_type: event_type.to_string(),
        span: el_span,
    };
    let alloc = Allocator::default();
    let wrapped = format!("({handler_source});");
    let Some(program) = reparse_module(&alloc, &wrapped) else {
        // Unparseable handler — refuse (never emit raw).
        return Err(refuse());
    };
    let Some(Statement::ExpressionStatement(stmt)) = program.body.first() else {
        return Err(refuse());
    };
    // Unwrap the synthetic parenthesization to reach the handler expression.
    let mut expr = &stmt.expression;
    while let Expression::ParenthesizedExpression(p) = expr {
        expr = &p.expression;
    }
    let Expression::ArrowFunctionExpression(arrow) = expr else {
        // A function expression, a bare identifier (local-function / import), a call,
        // an update, a member, a sequence, a conditional — the wrapper form (5d).
        return Err(refuse());
    };
    if arrow.r#async {
        return Err(UnsupportedSvelteRuntimeSurface::ExperimentalAsync {
            surface: "async event handler",
            span: el_span,
        });
    }
    // The arrow must take NO parameters (a `(e) => …` handler reads the event arg —
    // the broader handler-arg surface is a deferral). The §1.2-class handler is
    // nullary.
    if !arrow.params.items.is_empty() || arrow.params.rest.is_some() {
        return Err(refuse());
    }
    // The body must be EXCLUSIVELY `$state` assignment / update statements: either an
    // expression body that IS a `$state` assignment / update, or a block whose every
    // statement is a `$state` assignment / update expression statement.
    if arrow_body_is_state_writes(arrow, scope, bindings, scopes) {
        Ok(ClientEventHandlerShape::Arrow)
    } else {
        Err(refuse())
    }
}

/// Whether an arrow handler's body is EXCLUSIVELY `$state` assignment / update
/// statements (the supported §1.2-class onclick body). An EXPRESSION-bodied arrow
/// (`() => count += 1`) must have a `$state` assignment / update expression; a
/// BLOCK-bodied arrow (`() => { a++; b++; }`) must have a non-empty body whose every
/// statement is a `$state` assignment / update expression statement.
///
/// Decided structurally over the parsed arrow + the scope-aware binding table — a
/// call, a declaration, an `if`, an update of a non-`$state` (a plain local / prop /
/// derived), or an empty block all fail (drive the handler to the 5d wrapper form).
fn arrow_body_is_state_writes(
    arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> bool {
    // An expression-bodied arrow lowers its single expression into the function
    // body as a return/expression statement; OXC models it as a one-statement body
    // flagged `expression`.
    if arrow.r#expression {
        let [Statement::ExpressionStatement(stmt)] = arrow.body.statements.as_slice() else {
            return false;
        };
        return expr_is_state_write(&stmt.expression, scope, bindings, scopes);
    }
    // A block-bodied arrow: a non-empty body whose every statement is a `$state`
    // assignment / update expression statement.
    if arrow.body.statements.is_empty() {
        return false;
    }
    arrow.body.statements.iter().all(|stmt| {
        matches!(stmt, Statement::ExpressionStatement(es)
            if expr_is_state_write(&es.expression, scope, bindings, scopes))
    })
}

/// Whether an expression is a `$state` assignment / update: an `AssignmentExpression`
/// or `UpdateExpression` whose TARGET is a bare identifier resolving to a reactive
/// `$state` signal binding (a primitive `StateSignal`). A member target, a target
/// resolving to a plain local / prop / derived, or any other expression is NOT a
/// supported `$state` write.
fn expr_is_state_write(
    expr: &Expression<'_>,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> bool {
    let target_name = match expr {
        Expression::AssignmentExpression(assign) => assignment_target_ident(&assign.left),
        Expression::UpdateExpression(update) => simple_target_ident(&update.argument),
        _ => return false,
    };
    let Some(name) = target_name else {
        return false;
    };
    matches!(
        bindings.resolve_kind(scopes, scope, &name),
        Some(BindingRuntimeKind::StateSignal { raw: false })
    )
}

/// The bare-identifier name of an assignment target (`count = …` → `count`), or
/// `None` for a member / destructure / TS-wrapped target.
fn assignment_target_ident(target: &oxc_ast::ast::AssignmentTarget<'_>) -> Option<String> {
    use oxc_ast::ast::AssignmentTarget;
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// The bare-identifier name of an update target (`count++` → `count`), or `None`
/// for a member / TS-wrapped target.
fn simple_target_ident(target: &oxc_ast::ast::SimpleAssignmentTarget<'_>) -> Option<String> {
    use oxc_ast::ast::SimpleAssignmentTarget;
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Interpolation shape
// ---------------------------------------------------------------------------

/// The accepted shape of a reactive interpolation expression.
///
/// The supported interpolation surface is a BARE `Identifier` read resolving to
/// EITHER a reactive `$state` signal (emitted `$.get(x)`) OR a no-default read-only
/// prop (emitted `$$props.x`). Every other expression shape — a binary / logical /
/// conditional / sequence / unary / assignment / update, a member / optional-member,
/// a call / optional-call, a `new`, an array / object / function / class / template /
/// tagged-template, a literal, `this`, `await`, a TS wrapper, a parenthesized — needs
/// the official `build_template_chunk` evaluator (the `has_call` memoizer, the
/// `is_defined` nullish-coalesce, the parenthesization builder) and is a
/// reactive-text-completion follow-up, so it fails closed (5r).
// TODO(follow-up): port the official `build_template_chunk` evaluator (the `has_call`
// memoizer deps-array `$.template_effect`, the `is_defined` nullish-coalesce, the
// parenthesization builder) so a binary / call / member / conditional interpolation
// lowers instead of failing closed. Owned by the reactive-text-completion block (5r).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientInterpolationShape {
    /// `{x}` where `x` resolves to a reactive `$state` signal — emitted `$.get(x)`.
    SignalIdentRead,
    /// `{x}` where `x` resolves to a no-default read-only prop — emitted `$$props.x`.
    NoDefaultPropRead,
}

/// Classify a reactive interpolation's expression into its accepted
/// [`ClientInterpolationShape`], or fail closed.
///
/// Accepts ONLY a bare `Identifier` resolving (scope-awarely) to EITHER a reactive
/// `$state` signal binding (`StateSignal`/`StateProxy`/each/await/derived-as-signal)
/// OR a no-default `$props()` prop binding. A bare identifier resolving to a
/// NON-reactive binding (a plain local / module const) is the static-fold deferral
/// (5n). EVERY non-identifier expression shape (a binary, a call, a member, an
/// optional-call, a conditional, a literal, `this`, a parenthesized / TS-wrapped
/// read, …) needs the official `build_template_chunk` evaluator and fails closed
/// (5r) BY CONSTRUCTION — there is no wildcard accept. Drives the decision from the
/// typed parse + the scope-aware binding table; never a text scan.
pub(super) fn classify_interpolation_shape(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    span: Span,
) -> Result<ClientInterpolationShape, UnsupportedSvelteRuntimeSurface> {
    let refuse_complex = || UnsupportedSvelteRuntimeSurface::ComplexInterpolation { span };
    let alloc = Allocator::default();
    // Parse the RAW interpolation source as a statement (NOT wrapped in synthetic
    // parens — a wrap would make a bare `name` a `ParenthesizedExpression`). A bare
    // `name` is an `ExpressionStatement(Identifier)`; a user `(x)` / `x!` / `count +
    // 1` parses as a parenthesized / TS-wrapped / binary expression statement and is
    // refused.
    let Some(program) = reparse_module(&alloc, source) else {
        return Err(refuse_complex());
    };
    // Exactly one statement that IS an expression statement (a multi-statement source
    // is malformed for an interpolation).
    let [Statement::ExpressionStatement(stmt)] = program.body.as_slice() else {
        return Err(refuse_complex());
    };
    // The interpolation must be a BARE identifier. A parenthesized / TS-wrapped /
    // any other expression is the `build_template_chunk` breadth (5r) — the wrappers
    // are NOT unwrapped (a `{(x)}` / `{x!}` is a deferral, not the bare-read shape).
    let Expression::Identifier(id) = &stmt.expression else {
        return Err(refuse_complex());
    };
    match bindings.resolve_kind(scopes, scope, id.name.as_str()) {
        // A reactive `$state` signal read → `$.get(x)`.
        Some(k) if is_signal_binding(k) => Ok(ClientInterpolationShape::SignalIdentRead),
        // A no-default read-only prop read → `$$props.x`. (A DEFAULT-bearing prop is
        // already refused at the props-shape gate; a `BindableProp` is refused at the
        // binding-kind gate — so a `Prop` here is a no-default prop.)
        Some(BindingRuntimeKind::Prop) => Ok(ClientInterpolationShape::NoDefaultPropRead),
        // A bare identifier resolving to a NON-reactive binding (a plain local, a
        // module const, a never-reassigned `$state` lowered to PlainLet) is a
        // compile-time constant — official static-folds it to a `textContent` write,
        // a distinct topology (5n). A `BareProxy` (object/array `$state`) read is
        // refused at the binding-kind gate (5g) before reaching here.
        _ => Err(UnsupportedSvelteRuntimeSurface::StaticInterpolation { span }),
    }
}

// ---------------------------------------------------------------------------
// Text-run literal chunk shape
// ---------------------------------------------------------------------------

/// Whether a literal text node's SIGNIFICANT content is SIMPLE ASCII — the only
/// text-chunk shape the §1.2-class reactive-text / static-skeleton emission
/// serializes verbatim.
///
/// The node's leading / trailing HTML whitespace (` \t\r\n`) is BOUNDARY whitespace
/// — the official compiler trims / collapses it at a run boundary, so it does NOT
/// make a chunk complex (a pure-whitespace structural node between elements, or an
/// indented `\n  hello\n` chunk, is fine). The remaining SIGNIFICANT core must be
/// printable ASCII (0x20..=0x7E) with NO HTML entity reference (`&amp;`, `&#39;`,
/// …), NO interior tab / newline / carriage-return, NO repeated-space run (which the
/// official compiler whitespace-normalizes), and NO backtick / `${` / backslash
/// (which would need template-literal escaping). A core failing ANY of these needs
/// the official boundary-trimming / entity-decode / escaping path and fails closed
/// (5u).
// TODO(follow-up): apply the official boundary-trimming / `decode_character_references`
// / template-literal escaping path so an entity / interior-whitespace / escaping-need
// text chunk lowers instead of failing closed. Owned by the reactive-text-completion
// block (5u).
#[must_use]
pub(super) fn text_chunk_is_simple_ascii(chunk: &str) -> bool {
    // Strip the boundary HTML whitespace; a pure-whitespace (structural) node strips
    // to empty and is always simple. The SIGNIFICANT core is what must be simple.
    let core = chunk.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
    if core.is_empty() {
        return true;
    }
    let bytes = core.as_bytes();
    let mut prev_space = false;
    for (i, &b) in bytes.iter().enumerate() {
        // Only printable ASCII (0x20 space .. 0x7E `~`). A control char, an INTERIOR
        // tab / newline / CR, or any non-ASCII byte fails.
        if !(0x20..=0x7E).contains(&b) {
            return false;
        }
        // An HTML entity reference (`&…`) needs decoding — fail.
        if b == b'&' {
            return false;
        }
        // A backtick / backslash needs template-literal escaping — fail.
        if b == b'`' || b == b'\\' {
            return false;
        }
        // A `${` sequence is a template-literal interpolation opener — fail.
        if b == b'$' && bytes.get(i + 1) == Some(&b'{') {
            return false;
        }
        // A repeated-space run is whitespace-normalized by the official compiler —
        // fail (only single inter-word spaces serialize verbatim).
        let is_space = b == b' ';
        if is_space && prev_space {
            return false;
        }
        prev_space = is_space;
    }
    true
}

// ---------------------------------------------------------------------------
// Bind shape
// ---------------------------------------------------------------------------

/// The accepted shape of a supported `bind:` directive.
///
/// The supported boundary: `bind:value` on an `<input>` to a bare IDENTIFIER
/// resolving to a reactive `$state` signal, and `bind:this` on an intrinsic element
/// to a bare non-prop IDENTIFIER. A `bind:value` to a plain local, a PROP ident, a
/// member (`obj.x`), a non-lvalue (`{f()}`), a sequence get/set pair, a non-`input`
/// host, or a `bind:this` to a member / prop all fail closed (5c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientBindShape {
    /// `bind:value={ident}` on an `<input>`, the target a reactive signal binding.
    ValueSignalIdent,
    /// `bind:this={ident}` on an intrinsic element, the target a non-prop binding.
    ThisIdent,
}

/// Classify a supported `bind:` directive into its accepted [`ClientBindShape`], or
/// fail closed.
///
/// `target` is the bind target (`value` / `this`); `tag` is the host element tag;
/// `expr_source` is the bound expression source. Both supported binds REQUIRE an
/// explicit bound-expression source — a sourceless bind (`expr_source: None`) fails
/// closed, because runtime-op collection emits `$.bind_value` / `$.bind_this` only
/// for an `AttrIr::Bind { expr: Some(_) }`; accepting a sourceless bind would record
/// a shape the emitter then silently drops. (The shorthand `bind:value` reaches here
/// as `Some("value")` — its lowering synthesizes the bound `value` identifier.) The
/// scope-aware binding lookup resolves the target identifier's kind. ONLY a
/// `bind:value` on an `<input>` to a reactive `$state` signal IDENTIFIER and a
/// `bind:this` to a non-prop IDENTIFIER are accepted; a plain-local / prop / member /
/// non-lvalue / sourceless target fails closed (5c).
#[allow(clippy::too_many_arguments)]
pub(super) fn classify_bind_shape(
    target: &str,
    tag: &str,
    expr_source: Option<&str>,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    instance_locals: &rustc_hash::FxHashSet<String>,
    el_span: Span,
) -> Result<ClientBindShape, UnsupportedSvelteRuntimeSurface> {
    let refuse = || UnsupportedSvelteRuntimeSurface::Binding {
        target: target.to_string(),
        span: el_span,
    };
    // A TS-WRAPPED bind target (`bind:value={name!}` / `{(name!)}` / `{name as T}`)
    // is a distinct official surface (svelte strips the wrapper and emits the clean
    // lvalue setter; Verter would format the setter from the raw source, emitting an
    // invalid `name! = $$value`). The canonical-lvalue-from-TS lowering is a
    // deferral, so a TS-wrapped target fails closed (5c) — only a CLEAN identifier
    // lvalue is supported. Checked structurally over the parsed target, BEFORE the
    // lvalue classification (which unwraps the TS spine).
    if let Some(source) = expr_source {
        let alloc = Allocator::default();
        if bind_target_is_ts_wrapped(&alloc, source) {
            return Err(refuse());
        }
    }
    match target {
        // `bind:this` — an intrinsic element binding to a DECLARED non-prop IDENTIFIER
        // only (the §1.2-core shape-3 `let el;` local). A member `bind:this={refs[0]}`
        // / a prop target / a FREE-or-undeclared target is the deferral-ledger
        // member-bind / prop-bind / declared-target-completion form (5c).
        "this" => {
            // The shorthand `bind:this` is not valid Svelte; an explicit identifier
            // target is required.
            let Some(source) = expr_source else {
                return Err(refuse());
            };
            let alloc = Allocator::default();
            match classify_bind_target(&alloc, source) {
                Some(BindTargetKind::Identifier) => {
                    let name = source.trim();
                    // A FREE / UNDECLARED `bind:this` target is official-accepted but
                    // outside the core: official reserves a fresh local for it (`var
                    // button_1`), while Verter's element-local allocation would COLLIDE
                    // with the synthesized DOM local (a free `bind:this={button}` on a
                    // `<button>` aliases the DOM `button`). The supported shape is a
                    // DECLARED instance-script local (`let el;`), so a target that names
                    // no declared local fails closed (5c) — mooting the collision.
                    if !instance_locals.contains(name) {
                        return Err(refuse());
                    }
                    // A PROP target is the deferral-ledger prop-bind form (5c). A
                    // non-prop binding (a `$state` ref-target, a plain local) is the
                    // supported `bind:this` ident.
                    match bindings.resolve_kind(scopes, scope, name) {
                        Some(BindingRuntimeKind::Prop) | Some(BindingRuntimeKind::BindableProp) => {
                            Err(refuse())
                        }
                        _ => Ok(ClientBindShape::ThisIdent),
                    }
                }
                _ => Err(refuse()),
            }
        }
        // `bind:value` — an `<input>` only, to a reactive `$state` signal IDENTIFIER.
        // A textarea/select/checkbox/group bind, a plain-local / prop target, a
        // member, or a non-lvalue is the deferral-ledger bind vertical (5c).
        "value" if tag == "input" => {
            // An accepted `bind:value` REQUIRES an explicit bound-expression source
            // (an identifier). Runtime-op collection emits `$.bind_value` only for an
            // `AttrIr::Bind { expr: Some(_) }`; a bind whose lowering produced NO
            // bound expression (`expr_source: None`) would record an accepted shape
            // the emitter then silently drops (an accept-then-drop divergence — the
            // same class as the earlier `defaultValue` attr leak). So a sourceless
            // bind fails closed here: ACCEPTED == EMITTABLE. (The shorthand
            // `bind:value` is unaffected — its lowering synthesizes the bound `value`
            // identifier, reaching this classifier as `Some("value")`.)
            let Some(source) = expr_source else {
                return Err(refuse());
            };
            let alloc = Allocator::default();
            match classify_bind_target(&alloc, source) {
                Some(BindTargetKind::Identifier) => {
                    // ONLY a reactive `$state` signal target is supported (sets the
                    // signal via `$.set`). A plain local (a `name = $$value` direct
                    // assign), a PROP / `$bindable` target (the flag-7 2-arg
                    // `$.bind_value` form), or a `$derived` memo is a deferral (5c).
                    let resolved = bindings.resolve_kind(scopes, scope, source.trim());
                    match resolved {
                        Some(k) if is_signal_binding(k) => Ok(ClientBindShape::ValueSignalIdent),
                        _ => Err(refuse()),
                    }
                }
                // A member target (`obj.x` / `a[i]`), a non-lvalue (`bind:value={f()}`),
                // or a sequence get/set pair fails closed (5c).
                _ => Err(refuse()),
            }
        }
        // Every other bind target (`checked`, `group`, `value` on a non-input, …) is
        // the deferral-ledger bind vertical (5c).
        _ => Err(refuse()),
    }
}

/// Whether a binding kind is a reactive SIGNAL (a `bind:value` to it sets the
/// signal directly via `$.set`).
fn is_signal_binding(kind: BindingRuntimeKind) -> bool {
    matches!(
        kind,
        BindingRuntimeKind::StateSignal { .. }
            | BindingRuntimeKind::StateProxy
            | BindingRuntimeKind::Derived
            | BindingRuntimeKind::EachSignal
            | BindingRuntimeKind::AwaitSignal
            | BindingRuntimeKind::LegacyConstDerived
    )
}

// ---------------------------------------------------------------------------
// Dynamic attribute / class / style shape
// ---------------------------------------------------------------------------

/// The accepted emission shape of a supported dynamic attribute / `class` / `style`
/// surface . The classifier records this typed fact per accepted attribute
/// so the plan/emitter reads a proven emission decision, never re-derives it from a
/// raw name. Every shape mirrors a pinned `svelte@5.56.3` client form.
///
/// The DOM-property-vs-`set_attribute` decision is the official
/// `is_dom_property(normalize_attribute(name))` rule (the pinned tables in
/// [`super::client_allowlist`]); the `class` / `style` / `autofocus` arms are the
/// official special cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientDynamicAttrShape {
    /// A generic dynamic attribute → `$.set_attribute(node, 'name', value)` (the
    /// name is NOT a DOM property). E.g. `hidden={x}`, `contenteditable={x}`,
    /// `data-id={x}`, `aria-label={x}`, `id={x}`.
    SetAttribute {
        /// The (HTML-lowercased) attribute name the official skeleton serializer
        /// emits.
        name: String,
    },
    /// A dynamic DOM-property attribute → `node.<prop> = value` (the official
    /// `is_dom_property` rule). `prop` is the normalized property name
    /// (`disabled` → `disabled`, `readonly` → `readOnly`, `muted` → `muted`).
    DomProperty {
        /// The DOM property name to assign (already `normalize_attribute`'d).
        prop: String,
    },
    /// An `autofocus` attribute → init-only `$.autofocus(node, value)`. Both the
    /// static valueless (`$.autofocus(node, true)`) and dynamic
    /// (`$.autofocus(node, $.get(v))`) forms map here.
    Autofocus,
    /// A `class={…}` attribute or a `class:` directive — coalesced into one
    /// `$.set_class` by the plan.
    Class,
    /// A `style={…}` attribute or a `style:` directive — coalesced into one
    /// `$.set_style` by the plan.
    Style,
}

/// The dynamic-attribute names the client backend DEFERS to a later vertical (so the
/// generic open-set arm does not silently mis-emit them). Form-control value/checked
/// setters (`$.set_value` / `$.set_checked` / `$.set_selected` / `$.set_default_*`)
/// are the bindings-breadth surface (5c); `defaultValue` / `defaultChecked` are the
/// non-static-property form-default family (also 5c). The decision keys on the
/// NORMALIZED name so `defaultvalue` / `defaultValue` both match.
fn dynamic_attr_deferred_to_5c(normalized: &str) -> bool {
    matches!(
        normalized,
        "value" | "checked" | "selected" | "defaultValue" | "defaultChecked"
    )
}

/// Classify a supported dynamic PLAIN attribute (`AttrIr::Dynamic` / `AttrIr::Mixed`)
/// or a non-static-property attribute (`autofocus` / `muted` / …) into its accepted
/// [`ClientDynamicAttrShape`], or fail closed.
///
/// The classifier order mirrors the official `RegularElement.js` attribute dispatch:
///
/// 1. A `value` / `checked` / `selected` / `defaultValue` / `defaultChecked` name is
///    the form-control setter family (5c) — fail closed FIRST (so a `value={v}`
///    reports the form-control deferral, not the generic accept).
/// 2. An `is` attribute is the customized-built-in surface (5h) — but it is already
///    refused at the element gate, so it never reaches here.
/// 3. `autofocus` → [`ClientDynamicAttrShape::Autofocus`].
/// 4. `dir` is the special reflected-attr arm (`el.dir = el.dir`) — DEFERRED (a
///    follow-up), fail closed so the generic arm does not mis-emit it without the
///    reflection write.
/// 5. `is_dom_property(normalize_attribute(name))` → [`ClientDynamicAttrShape::DomProperty`]
///    (`muted` is a DOM property on ANY element — `is_dom_property` is element-agnostic).
/// 6. everything else → the generic [`ClientDynamicAttrShape::SetAttribute`].
///
/// `name` is the raw attribute name. `is_html` is always true for the supported
/// surface (an SVG/MathML element fails closed at the element gate).
pub(super) fn classify_dynamic_attr_shape(
    name: &str,
    el_span: Span,
) -> Result<ClientDynamicAttrShape, UnsupportedSvelteRuntimeSurface> {
    let refuse_dynamic = || UnsupportedSvelteRuntimeSurface::DynamicAttribute {
        name: name.to_string(),
        span: el_span,
    };
    // The official `get_attribute_name` for an HTML element is `normalize_attribute`
    // — lowercase + the alias map (`readonly` → `readOnly`). The supported surface is
    // HTML-only (an SVG/MathML element fails closed at the element gate), so the
    // normalized name is the property/attribute name the official compiler dispatches
    // on.
    let normalized = super::client_allowlist::normalize_attribute(name);
    // (1) The form-control setter family (`value` / `checked` / `selected` /
    // `defaultValue` / `defaultChecked`) is the bindings-breadth surface (5c) — it
    // emits the dedicated `$.set_value` / `$.set_checked` / `$.set_selected` /
    // `$.set_default_*` form helpers, alongside `bind:value` / `bind:checked`. Refuse
    // it through the 5c-owning `Binding` channel (the form-control attribute IS the
    // target a `bind:` would write), so the diagnostic carries the right owning block.
    if dynamic_attr_deferred_to_5c(&normalized) {
        return Err(UnsupportedSvelteRuntimeSurface::Binding {
            target: normalized,
            span: el_span,
        });
    }
    // (3) `autofocus` → the init-only helper.
    if normalized == "autofocus" {
        return Ok(ClientDynamicAttrShape::Autofocus);
    }
    // (4) `dir` is the special reflected-attr arm — DEFERRED.
    // TODO(follow-up): emit the official `dir` reflected-attr arm (`el.dir = el.dir`
    // pushed into the combined effect, on top of the static-bake / `$.set_attribute`)
    // — the Firefox `dir="auto"` direction fix (`RegularElement.js`'s
    // `if (lookup.has('dir'))`). Until then a `dir` attribute fails closed so the
    // generic arm never mis-emits it without the reflection write.
    if normalized == "dir" {
        return Err(refuse_dynamic());
    }
    // (5) A DOM property → a direct property write.
    if super::client_allowlist::is_dom_property(&normalized) {
        return Ok(ClientDynamicAttrShape::DomProperty { prop: normalized });
    }
    // (6) The generic open-set arm → `$.set_attribute`. The emitted attribute name is
    // the HTML-lowercased form (the official `template.js` serializer lowercases an
    // HTML attribute name; `set_attribute` receives the normalized name, which for a
    // non-DOM-property non-alias attribute is its lowercase spelling).
    Ok(ClientDynamicAttrShape::SetAttribute { name: normalized })
}

// ---------------------------------------------------------------------------
// $props() usage shape (read-only vs written/bound)
// ---------------------------------------------------------------------------

/// The accepted `$props()` usage fact — the props are READ-ONLY (no instance-script
/// write, no template write-ref, no `bind:` target resolves to a prop local).
///
/// A written prop (official's flag-7 setter-call form) or a bound prop (official's
/// 2-arg `$.bind_value(input, label)` form) is a deferral-ledger follow-up — both
/// fail closed BEFORE `lower_props_declarator`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientPropsUsage {
    /// The prop local names (the destructure locals), retained as the accepted fact.
    pub(super) prop_locals: Vec<String>,
}

/// Collect the `$props()` destructure local names from an instance script (the
/// LOCAL binding names, which may be aliases of the source keys). Empty when there
/// is no `$props()` destructure.
pub(super) fn collect_prop_locals(instance_source: Option<&str>) -> Vec<String> {
    let mut locals = Vec::new();
    let Some(src) = instance_source else {
        return locals;
    };
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, src) else {
        return locals;
    };
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let Some(Expression::CallExpression(call)) = &d.init else {
                continue;
            };
            if !super::expr::is_props_callee(&call.callee) {
                continue;
            }
            if let BindingPattern::ObjectPattern(obj) = &d.id {
                for prop in &obj.properties {
                    match &prop.value {
                        BindingPattern::BindingIdentifier(id) => locals.push(id.name.to_string()),
                        BindingPattern::AssignmentPattern(assign) => {
                            if let BindingPattern::BindingIdentifier(id) = &assign.left {
                                locals.push(id.name.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    locals
}

// ---------------------------------------------------------------------------
// Rune-declarator kind gate
// ---------------------------------------------------------------------------

/// Fail closed for any TOP-LEVEL rune declarator (`$state` / `$state.raw` /
/// `$derived` / `$derived.by` / `$props`) whose declaration keyword is NOT `let`.
///
/// Only a `let` rune declarator is supported. `var` / `const` rune declarators are
/// a DISTINCT official surface that Verter does not emit (5g):
///
/// - a `var` `$state` / `$derived` read is `$.safe_get(name)` (the `var`-hoisting
///   form), NOT `$.get(name)`;
/// - a read-only `const $state` compiles to an EMPTY reactive topology in official
///   (the value is constant-folded), not a `$.state(...)` signal;
/// - a `var` / `const` `$props()` declarator preserves the keyword on the emitted
///   declarator (`var a = $.prop(...)`), whereas Verter's props lowering hardcodes
///   `let`.
///
/// The declaration KIND is read from the parent `VariableDeclaration.kind` — a
/// structural decision over the parsed program, never a text scan. Verified against
/// svelte@5.56.3.
// TODO(follow-up): lower the non-`let` rune-declarator forms instead of failing
// closed — a `var` `$state`/`$derived` read selects `$.safe_get(name)` (the
// var-hoisting helper), a read-only `const $state` constant-folds to its init (no
// signal), and a `var`/`const` `$props()` declarator preserves the keyword on the
// emitted `$.prop(...)` declarator. Until then a non-`let` rune declarator is the
// deferral-ledger refusal below.
pub(super) fn classify_rune_declaration_kind(
    instance_source: &str,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance_source) else {
        return Ok(());
    };
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        if decl.kind == VariableDeclarationKind::Let {
            continue;
        }
        // A non-`let` declaration: fail closed if ANY declarator initializes a rune
        // call. A non-rune `var`/`const` local stays supported (the keyword is
        // emitted faithfully for a plain local).
        for d in &decl.declarations {
            let Some(Expression::CallExpression(call)) = &d.init else {
                continue;
            };
            let rune = if state_rune_call(call).is_some() {
                "non-let $state declarator"
            } else if is_derived_callee(&call.callee) {
                "non-let $derived declarator"
            } else if is_props_callee(&call.callee) {
                "non-let $props() declarator"
            } else {
                continue;
            };
            return Err(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune,
                span: Span::new(decl.span.start, decl.span.end),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Instance-script item allowlist (the strict finite supported-shape set)
// ---------------------------------------------------------------------------

/// A TYPED supported instance-script item — the closed allowlist of top-level
/// instance-script declaration shapes the client core lowers. This is the
/// script-side analogue of [`SupportedHtmlElement`](super::client_allowlist::SupportedHtmlElement):
/// the classifier ([`classify_supported_instance_items`]) admits ONLY these three
/// shapes and the lowering ([`super::expr_emit::lower_supported_instance_items`])
/// consumes ONLY this enum — there is NO "emit any non-rune statement" path. Every
/// OTHER top-level item (a function / class / enum / namespace / interface / type /
/// plain non-rune `let`-`const`-`var` / arbitrary statement / `$:` label /
/// `$`-`$$`-prefixed binding) fails closed BY CONSTRUCTION at the classifier.
///
/// Each variant carries the FULLY-RESOLVED lowering inputs (a binding name, the
/// init payload text), so the lowering is a thin per-variant transform that never
/// re-walks an arbitrary statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SupportedInstanceScriptItem {
    /// `let name = $state(<primitive literal>);` — one declarator, `let` only,
    /// identifier binding, no TS annotation, a 0-1-arg `$state()` with a primitive
    /// literal init. Carries the binding name and the init payload (the primitive
    /// literal source text, or `None` for the no-arg `$state()` ⇒ `void 0` form).
    StatePrimitive {
        /// The declared signal name.
        name: String,
        /// The primitive-literal init source text (`'world'`, `0`, `-1`, `true`,
        /// `null`, …), or `None` for the no-arg `$state()` form.
        init: Option<String>,
    },
    /// A single no-default `$props()` destructure (`let { a } = $props()` /
    /// `let { a: b } = $props()`). A no-default destructure emits NO component-body
    /// declaration (the props are read directly off `$$props`), so this variant
    /// carries no lowering payload — it is the classification fact that the props
    /// destructure was accepted (the props reads are projected separately).
    PropsDestructure,
    /// `let el;` — a bare (no-init, no-annotation) `let` identifier used SOLELY as a
    /// supported `bind:this` target. Carries the binding name (lowered to `let el;`).
    BindThisLocal {
        /// The declared local name.
        name: String,
    },
}

/// Classify the instance script's TOP-LEVEL items into the strict finite
/// [`SupportedInstanceScriptItem`] allowlist, or fail closed on the FIRST
/// out-of-allowlist item.
///
/// The three supported shapes are EXACTLY:
/// 1. `let name = $state(<primitive literal>);`
/// 2. a single no-default `$props()` destructure;
/// 3. `let el;` used solely as a supported `bind:this` target.
///
/// `bind_this_targets` is the set of local names used as a supported `bind:this`
/// target (from the accepted bind shapes) — a bare `let el;` is admitted ONLY when
/// its name is in this set; an unused / plain bare local fails closed.
///
/// Everything else fails closed: a plain `let x = 0`, a `const` / `var`, a top-level
/// function / class / enum / namespace / interface / type, an arbitrary expression /
/// control-flow / empty statement, a `$:` reactive label, an import / export, a
/// `$` / `$$`-prefixed binding, and the magic refs `$$slots` / `$$props` /
/// `$$restProps`. The decision is driven from the typed OXC AST (statement kind,
/// declarator pattern, init shape, TS-annotation presence), never a text scan.
///
/// Two whole-program pre-passes run FIRST so their PRECISE diagnostics win over the
/// generic item refusal: the rune-form / rune-position scan (owned by
/// [`super::client_surface`]) and the magic-identifier scan ([`scan_magic_identifiers`]).
pub(super) fn classify_supported_instance_items(
    instance_source: &str,
    bind_this_targets: &[String],
) -> Result<Vec<SupportedInstanceScriptItem>, UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance_source) else {
        // An unparseable instance script is recorded as a script-parse diagnostic
        // upstream; classify yields no items (the upstream parse gate owns the refusal).
        return Ok(Vec::new());
    };

    let mut items = Vec::new();
    for stmt in &program.body {
        items.push(classify_instance_statement(
            stmt,
            instance_source,
            bind_this_targets,
        )?);
    }
    Ok(items)
}

/// Classify ONE top-level instance-script statement into its supported item, or
/// fail closed. The supported statements are EXACTLY a `let`-variable declaration
/// matching shape 1/2/3; every other statement kind fails closed with a precise
/// `construct` label.
fn classify_instance_statement(
    stmt: &Statement<'_>,
    instance_source: &str,
    bind_this_targets: &[String],
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    match stmt {
        Statement::VariableDeclaration(decl) => {
            classify_instance_variable_decl(decl, instance_source, bind_this_targets)
        }
        // Every NON-variable top-level statement fails closed with its construct
        // label. The labels are precise so the completeness gate can pin each family.
        other => Err(UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
            construct: top_level_statement_label(other),
            span: stmt_span(other),
        }),
    }
}

/// Classify a top-level `VariableDeclaration` into shape 1/2/3, or fail closed.
///
/// A `var` / `const` declaration, a multi-declarator declaration, or any declarator
/// that is not exactly one of the three supported shapes fails closed.
fn classify_instance_variable_decl(
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    instance_source: &str,
    bind_this_targets: &[String],
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = |construct: &'static str| UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct,
        span: Span::new(decl.span.start, decl.span.end),
    };
    // (1) `let` ONLY — a `const` / `var` declaration is a distinct official surface
    // (`var` reads use `$.safe_get`, a read-only `const $state` constant-folds), and
    // a plain `const`/`var` local is not core. Fail closed.
    if decl.kind != VariableDeclarationKind::Let {
        return Err(refuse(match decl.kind {
            VariableDeclarationKind::Const => "const declaration",
            VariableDeclarationKind::Var => "var declaration",
            _ => "non-let declaration",
        }));
    }
    // (2) EXACTLY ONE declarator — a multi-declarator `let a = $state(0), b = 1;`
    // mixes shapes and is not core. Fail closed.
    let [d] = decl.declarations.as_slice() else {
        return Err(refuse("multi-declarator let"));
    };
    // (3) NO TS annotation — `let c: number = $state(0)` / a definite `let c!: T`
    // is a TS-leniency form (a plain `<script>` parsed as TSX accepts the
    // annotation). The supported shapes carry NO annotation. Fail closed.
    if d.type_annotation.is_some() || d.definite {
        return Err(refuse("ts-annotated let"));
    }
    // (4) The binding name (an identifier pattern). A destructure pattern is handled
    // by the `$props()` shape below; an array pattern / non-identifier non-props
    // declarator is not core.
    match &d.id {
        BindingPattern::BindingIdentifier(id) => {
            let name = id.name.as_str();
            // A `$` / `$$`-prefixed binding (`let $$anchor`, `let $foo`) is reserved
            // (the `$$`-prefix is the compiler-magic namespace; the `$`-prefix is the
            // store-subscription namespace). Fail closed BEFORE the init shape.
            if name.starts_with('$') {
                return Err(refuse("$-prefixed binding"));
            }
            classify_identifier_declarator(d, name, decl, instance_source, bind_this_targets)
        }
        BindingPattern::ObjectPattern(_) => {
            // The ONLY supported destructure is a no-default `$props()` call. The
            // detailed shape (no defaults / rest / computed / nested / `$bindable`)
            // is enforced by `props_shape` upstream; here the declarator must be a
            // `$props()` call destructure.
            let Some(Expression::CallExpression(call)) = &d.init else {
                return Err(refuse("object-destructure let"));
            };
            if !is_props_callee(&call.callee) {
                return Err(refuse("object-destructure let"));
            }
            Ok(SupportedInstanceScriptItem::PropsDestructure)
        }
        BindingPattern::ArrayPattern(_) => Err(refuse("array-destructure let")),
        BindingPattern::AssignmentPattern(_) => Err(refuse("default-pattern let")),
    }
}

/// Classify a `let <ident> …` declarator (the identifier already known non-`$`-prefixed)
/// into shape 1 (`$state(<primitive>)`) or shape 3 (bare `let el;` bind:this target),
/// or fail closed.
fn classify_identifier_declarator(
    d: &oxc_ast::ast::VariableDeclarator<'_>,
    name: &str,
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    instance_source: &str,
    bind_this_targets: &[String],
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = |construct: &'static str| UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct,
        span: Span::new(decl.span.start, decl.span.end),
    };
    match &d.init {
        // Shape 3: a bare `let el;` (no init) — admitted ONLY when used solely as a
        // supported `bind:this` target. An unused / plain bare local fails closed.
        None => {
            if bind_this_targets.iter().any(|t| t == name) {
                Ok(SupportedInstanceScriptItem::BindThisLocal {
                    name: name.to_string(),
                })
            } else {
                Err(refuse("unused bare let"))
            }
        }
        // Shape 1: `let name = $state(<primitive literal>)`. The `$state` family,
        // arity (0-1), and primitive-literal init are validated here; the destructure
        // / non-primitive / multi-arg / `$state.raw` forms are owned by the upstream
        // `state_decl_shape` gate (which fails them as `AdvancedRune`), so on the
        // accept path a `$state(<primitive>)` identifier declarator reaches here.
        Some(Expression::CallExpression(call)) => {
            // A `$state` / `$state.raw` call.
            if state_rune_call(call).is_some() {
                // The init payload: the primitive-literal source text, or `None`
                // for the no-arg `$state()` form (lowered to `void 0`).
                let init = state_primitive_init_text(call, instance_source);
                return Ok(SupportedInstanceScriptItem::StatePrimitive {
                    name: name.to_string(),
                    init,
                });
            }
            // A `$derived` / `$props()` / other call init for an IDENTIFIER binding
            // is not a supported shape (a `$derived` identifier is a deferral; a
            // `$props()` identifier is a whole-object binding). Fail closed.
            if is_derived_callee(&call.callee) {
                return Err(refuse("$derived declarator"));
            }
            if is_props_callee(&call.callee) {
                return Err(refuse("$props() whole-object"));
            }
            // A plain non-rune call init (`let x = makeIt()`) is not core.
            Err(refuse("plain let with call init"))
        }
        // A plain non-rune `let x = 0` (a literal / object / array / member / …
        // init) is NOT core — a template read is only a reactive `$state` signal or
        // a no-default prop, never a plain local. Fail closed.
        Some(_) => Err(refuse("plain let")),
    }
}

/// The primitive-literal init source text of a `$state(<arg>)` call, or `None` for
/// the no-arg `$state()` form. A primitive literal carries NO signal read and NO TS
/// syntax, so its source slice is emitted verbatim (matching official). The
/// over-arity / non-primitive forms are refused upstream, so the first argument is a
/// primitive literal here. The argument span is absolute into `instance_source` (the
/// SAME buffer the program was parsed from), so the slice is the exact user text.
fn state_primitive_init_text(
    call: &oxc_ast::ast::CallExpression<'_>,
    instance_source: &str,
) -> Option<String> {
    use oxc_span::GetSpan;
    let arg = call.arguments.first()?.as_expression()?;
    let span = arg.span();
    Some(instance_source[span.start as usize..span.end as usize].to_string())
}

/// Scan an instance-script (or template-expression) program for a compiler-MAGIC
/// identifier reference (`$$slots` / `$$props` / `$$restProps`). Returns the FIRST
/// magic-identifier surface, or `None`. A LOCAL binding shadowing the name (a
/// function param / nested `let` of the same name) is NOT a magic reference — the
/// scan reuses the shared lexical [`super::expr::ShadowStack`] model so the
/// shadowing semantics match the rune scan.
pub(super) fn scan_magic_identifiers(source: &str) -> Option<UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let program = reparse_module(&alloc, source)?;
    let mut scan = MagicIdentScan {
        scopes: super::expr::ShadowStack::default(),
        found: None,
    };
    use oxc_ast_visit::Visit;
    scan.visit_program(&program);
    scan.found
}

/// The Svelte compiler-MAGIC identifier names (the auto-injected legacy magic
/// objects). A reference to one of these in the runes client output would bind an
/// undefined identifier (a runtime `ReferenceError`).
const MAGIC_IDENT_NAMES: &[&str] = &["$$slots", "$$props", "$$restProps"];

/// The scope-aware scan state for a magic-identifier reference.
struct MagicIdentScan {
    scopes: super::expr::ShadowStack,
    found: Option<UnsupportedSvelteRuntimeSurface>,
}

impl<'a> oxc_ast_visit::Visit<'a> for MagicIdentScan {
    fn visit_program(&mut self, it: &oxc_ast::ast::Program<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        super::expr::collect_direct_decls(&it.body, &mut frame);
        super::expr::collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        oxc_ast_visit::walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.scopes.push(super::expr::function_scope_names(it));
        oxc_ast_visit::walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.scopes.push(super::expr::arrow_scope_names(it));
        oxc_ast_visit::walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.scopes.push(super::expr::block_scope_names(it));
        oxc_ast_visit::walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        let name = it.name.as_str();
        if self.found.is_none()
            && MAGIC_IDENT_NAMES.contains(&name)
            && !self.scopes.is_shadowed(name)
        {
            let magic: &'static str = match name {
                "$$slots" => "$$slots",
                "$$props" => "$$props",
                "$$restProps" => "$$restProps",
                _ => "$$magic",
            };
            self.found = Some(UnsupportedSvelteRuntimeSurface::MagicIdentifier {
                name: magic,
                span: Span::new(it.span.start, it.span.end),
            });
        }
        oxc_ast_visit::walk::walk_identifier_reference(self, it);
    }
}

/// A short construct label for a top-level instance-script statement that is NOT a
/// variable declaration. Each kind gets a precise label so the completeness gate
/// pins the family (function / class / enum / namespace / interface / type / `$:` /
/// import / export / expression / control-flow / empty / …).
fn top_level_statement_label(stmt: &Statement<'_>) -> &'static str {
    match stmt {
        Statement::FunctionDeclaration(_) => "function",
        Statement::ClassDeclaration(_) => "class",
        Statement::TSEnumDeclaration(_) => "enum",
        Statement::TSModuleDeclaration(_) => "namespace",
        Statement::TSInterfaceDeclaration(_) => "interface",
        Statement::TSTypeAliasDeclaration(_) => "type alias",
        Statement::TSImportEqualsDeclaration(_) => "import-equals",
        Statement::LabeledStatement(_) => "$: label",
        Statement::ImportDeclaration(_) => "import",
        Statement::ExportNamedDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::ExportDefaultDeclaration(_) => "export",
        Statement::ExpressionStatement(_) => "expression statement",
        Statement::IfStatement(_)
        | Statement::ForStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::WhileStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::SwitchStatement(_)
        | Statement::TryStatement(_)
        | Statement::BlockStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_)
        | Statement::WithStatement(_) => "control-flow statement",
        Statement::EmptyStatement(_) => "empty statement",
        Statement::DebuggerStatement(_) => "debugger statement",
        // Any other statement kind (a `using` declaration, …) is still
        // out-of-allowlist.
        _ => "instance-script statement",
    }
}

/// The verter span of a top-level statement (for the fail-closed diagnostic).
fn stmt_span(stmt: &Statement<'_>) -> Span {
    use oxc_span::GetSpan;
    let span = stmt.span();
    Span::new(span.start, span.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svelte::runtime::expr::BindingInfo;

    /// A scope graph with a single root holding one `$state` signal binding named
    /// `value` (the reactive-signal target a `bind:value` resolves to).
    fn signal_value_env() -> (BindingTable, ScopeGraph, ScopeId) {
        let (mut graph, root) = ScopeGraph::with_root();
        let mut bindings = BindingTable::new();
        let id = bindings.push(BindingInfo {
            name: "value".to_string(),
            scope: root,
            kind: BindingRuntimeKind::StateSignal { raw: false },
            state: None,
        });
        graph.declare(root, "value", id);
        (bindings, graph, root)
    }

    #[test]
    fn classify_bind_value_requires_an_explicit_bound_expression_source() {
        // ACCEPTED == EMITTABLE: a `bind:value` with NO bound-expression source
        // (`expr_source: None`) must FAIL CLOSED. Runtime-op collection only emits
        // `$.bind_value` for an `AttrIr::Bind { expr: Some(_) }`; a classifier that
        // accepted a sourceless bind (the old `expr_source.unwrap_or("value")`
        // fabrication) would record a bind shape the emitter then silently drops —
        // an accept-then-drop divergence. The fix makes the absence of a bound
        // expression a refusal at the classifier, so an accepted bind shape ALWAYS
        // has an emittable expression.
        let (bindings, scopes, root) = signal_value_env();
        let locals = rustc_hash::FxHashSet::default();
        let span = Span::new(0, 0);
        let res = classify_bind_shape(
            "value", "input", /* expr_source = */ None, root, &bindings, &scopes, &locals,
            span,
        );
        assert!(
            matches!(
                res,
                Err(UnsupportedSvelteRuntimeSurface::Binding { ref target, .. }) if target == "value"
            ),
            "a sourceless `bind:value` must fail closed as the `bind:value` surface, got {res:?}"
        );
        // NEGATIVE: it must NOT be accepted as any bind shape (the pre-fix
        // `unwrap_or(\"value\")` accepted it as `ValueSignalIdent`).
        assert!(
            res.is_err(),
            "a sourceless `bind:value` must NOT be accepted (accept-then-drop): {res:?}"
        );
    }

    #[test]
    fn classify_bind_value_accepts_an_explicit_signal_identifier_source() {
        // The positive boundary: an EXPLICIT bound identifier resolving to a signal
        // is accepted (the §1.2 surface + the synthesized-shorthand `value` source
        // both reach the classifier as a `Some(_)` identifier source).
        let (bindings, scopes, root) = signal_value_env();
        let locals = rustc_hash::FxHashSet::default();
        let span = Span::new(0, 0);
        let res = classify_bind_shape(
            "value",
            "input",
            /* expr_source = */ Some("value"),
            root,
            &bindings,
            &scopes,
            &locals,
            span,
        );
        assert_eq!(
            res,
            Ok(ClientBindShape::ValueSignalIdent),
            "an explicit `bind:value={{value}}` to a $state signal must be accepted"
        );
    }

    #[test]
    fn classify_bind_this_requires_a_declared_local_target() {
        // A `bind:this={el}` where `el` IS a declared instance-script local is the
        // supported shape-3 target (accepted); a FREE `bind:this={button}` (no declared
        // local) fails closed (5c) — official accepts it but reserves a fresh local,
        // whereas Verter's element-local allocation would collide with the synthesized
        // DOM local, so the free target is refused to moot the collision.
        let (bindings, scopes, root) = signal_value_env();
        let span = Span::new(0, 0);
        let mut locals = rustc_hash::FxHashSet::default();
        locals.insert("el".to_string());

        // DECLARED target `el` (a bare local, not a binding-table row) — accepted.
        let declared = classify_bind_shape(
            "this",
            "div",
            Some("el"),
            root,
            &bindings,
            &scopes,
            &locals,
            span,
        );
        assert_eq!(
            declared,
            Ok(ClientBindShape::ThisIdent),
            "a declared `let el;` bind:this target is the supported shape-3"
        );

        // FREE target `button` (undeclared) — fails closed.
        let free = classify_bind_shape(
            "this",
            "button",
            Some("button"),
            root,
            &bindings,
            &scopes,
            &locals,
            span,
        );
        assert!(
            matches!(
                free,
                Err(UnsupportedSvelteRuntimeSurface::Binding { ref target, .. }) if target == "this"
            ),
            "a free / undeclared bind:this target must fail closed (5c): {free:?}"
        );
    }
}
