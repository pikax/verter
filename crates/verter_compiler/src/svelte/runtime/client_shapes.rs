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
    is_derived_callee, is_props_callee, reparse_module, state_rune_call, AnalyzedExpr,
    BindTargetKind, BindingRuntimeKind, BindingTable, ScopeGraph, ScopeId,
};
use super::ir::AttrIr;
use crate::svelte::bind_contract::{
    bind_target_policy, resolve_runtime_bind, BindTargetPolicy, RuntimeBindRouting, RuntimeHelper,
};
use verter_span::Span;

// ---------------------------------------------------------------------------
// Event-handler shape
// ---------------------------------------------------------------------------

/// The accepted shape of an event handler expression.
///
/// The handler is passed (rewritten) as the `$.event` / `$.delegated` 3rd positional
/// argument — wrapped in any legacy modifier wrappers. One accepted sub-shape:
///
/// - [`ClientEventHandlerShape::Inline`] — a non-async inline arrow / function
///   expression. The DELEGATED path admits ONLY the §1.2 nullary `$state`-write arrow
///   sub-shape (`() => count++`); the DIRECT (`$.event`) path admits ANY non-async
///   inline arrow / function (`() => {}`, `(e) => count++`), with its body lowered
///   through the shared expression rewriter (an unsupported body construct fails closed
///   at the rewrite, never emits raw).
///
/// Every OTHER handler shape — a bare identifier (`onfocus={ev}`), a member
/// (`onclick={o.m}`), a call, a conditional, an imported identifier — is not yet
/// supported and fails closed. A bare-identifier handler in particular needs the
/// official `build_event_handler` binding resolution (a non-import local /
/// declared-function lookup, or the `function (...$$args) { handler.apply(this, $$args);
/// }` wrapper / `$.derived` `has_call` hoist), which this surface does not own;
/// admitting it would emit the raw binding as a handler (`$.event(type, node, ident)`)
/// without the resolution that proves it is a function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientEventHandlerShape {
    /// A non-async inline arrow / function expression (rewritten).
    Inline,
}

/// Classify an event handler's expression into its accepted [`ClientEventHandlerShape`],
/// or fail closed.
///
/// `direct` selects the acceptance breadth: a DIRECT (`$.event`) handler — the
/// regular-element non-delegated / capture / legacy-modifier surface — admits any
/// non-async inline arrow / function expression; a DELEGATED (`$.delegated`) handler
/// keeps the NARROW §1.2 boundary (a nullary `$state`-write arrow only), so the
/// delegated path is unchanged. An async handler is the experimental-async surface; a
/// bare identifier / member / call / conditional / other expression is the official
/// binding-resolution / wrapper-form breadth (not yet supported) and fails closed. The
/// accepted surface is exactly the surface the committed `events/*` goldens prove.
pub(super) fn classify_event_handler_shape(
    handler_source: &str,
    event_type: &str,
    el_span: Span,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    direct: bool,
) -> Result<ClientEventHandlerShape, UnsupportedSvelteRuntimeSurface> {
    let refuse = || UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
        event_type: event_type.to_string(),
        span: el_span,
    };
    let async_refuse = || UnsupportedSvelteRuntimeSurface::ExperimentalAsync {
        surface: "async event handler",
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
    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            if arrow.r#async {
                return Err(async_refuse());
            }
            if direct {
                // DIRECT: any non-async inline arrow. The body lowers through the shared
                // rewriter at projection time (an unsupported construct fails closed
                // there).
                return Ok(ClientEventHandlerShape::Inline);
            }
            // DELEGATED (narrow §1.2): a NULLARY arrow whose body is EXCLUSIVELY `$state`
            // assignment / update statements.
            if !arrow.params.items.is_empty() || arrow.params.rest.is_some() {
                return Err(refuse());
            }
            if arrow_body_is_state_writes(arrow, scope, bindings, scopes) {
                Ok(ClientEventHandlerShape::Inline)
            } else {
                Err(refuse())
            }
        }
        // A function expression handler (`onclick={function () { … }}`) — the DIRECT
        // path admits it (passed through, body rewritten); the delegated narrow path
        // does not.
        Expression::FunctionExpression(func) if direct => {
            if func.r#async {
                return Err(async_refuse());
            }
            Ok(ClientEventHandlerShape::Inline)
        }
        // Every other handler shape — a bare identifier (`onfocus={ev}`, which needs the
        // official `build_event_handler` binding resolution this surface does not own), a
        // function expression on the delegated path, a member / call / conditional /
        // sequence — is the official binding-resolution / wrapper-form breadth and is not
        // yet supported. Fail closed (never emit the raw binding as a handler).
        _ => Err(refuse()),
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
/// `is_defined` nullish-coalesce, the parenthesization builder), which is owned by the
/// reactive-text/interpolation completion surface — so it fails closed here.
// TODO(follow-up): port the official `build_template_chunk` evaluator (the `has_call`
// memoizer deps-array `$.template_effect`, the `is_defined` nullish-coalesce, the
// parenthesization builder) so a binary / call / member / conditional interpolation
// lowers instead of failing closed. Owned by the reactive-text/interpolation completion
// surface (the global interpolation-breadth owner), not this declaration-tag surface.
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
/// NON-reactive binding (a plain local / module const) is the const-fold sub-contract
/// (the official `textContent` static fold). EVERY non-identifier expression shape (a
/// binary, a call, a member, an optional-call, a conditional, a literal, `this`, a
/// parenthesized / TS-wrapped read, …) needs the official `build_template_chunk` evaluator
/// (the reactive-text/interpolation completion surface) and fails closed BY CONSTRUCTION —
/// there is no wildcard accept. Drives the decision from the typed parse + the scope-aware
/// binding table; never a text scan.
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
    // any other expression is the `build_template_chunk` breadth (the reactive-text/
    // interpolation completion surface) — the wrappers are NOT unwrapped (a `{(x)}` /
    // `{x!}` is a deferral, not the bare-read shape).
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
        // a distinct topology (the const-fold sub-contract). A `BareProxy` (object/array
        // `$state`) read is refused at the binding-kind gate (5g) before reaching here.
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
/// The supported boundary (5c, DOM-hosted binds):
/// - `bind:this` on an intrinsic element to a bare non-prop IDENTIFIER, OR a
///   two-element getter/setter FUNCTION-PAIR (`bind:this={get, set}`) — both the
///   [`This`](Self::This) shape (discriminated by its [`BindGetSetForm`]).
/// - the DOM-value/property bind family on an ordinary DOM-element host — `value`
///   (`<input>`/`<textarea>`/`<select>`), `checked`, `group`, media
///   (`currentTime`/`paused`/`duration`/`played`/…), dimensions
///   (`clientWidth`/…), contenteditable (`innerHTML`/…), and the generic DOM
///   property (`open`/…) — each carrying the typed [`RuntimeBindRouting`] the
///   plan/emitter consume DATA-DRIVEN ([`DomBind`](Self::DomBind)). The bound
///   lvalue is a state signal / plain local / member (resolved at plan time);
///   a `$props()`-rooted / `$derived` / import-rooted target fails closed.
///
/// A `bind:value` to a PROP ident, a member rooted at a non-`$state` binding, a
/// non-lvalue (`{f()}`), a sequence target on an identifier/member-only bind
/// (`bind:group={get, set}`), a non-two-element sequence, an unsupported `(name, host)`
/// pair, or a `bind:this` to a member / prop all fail closed (5c). A two-element
/// function-pair `{get, set}` on a policy-allowed bind (`bind:value`/`bind:checked`/…)
/// IS admitted (`FunctionPair`); only identifier/member-only binds refuse a sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientBindShape {
    /// A DOM-hosted value/property bind (`value`/`checked`/`group`/media/dimension/
    /// contenteditable/property) on an ordinary DOM element, carrying the typed
    /// runtime emission routing (helper / arity / event / prelude). The bound
    /// lvalue's signal-vs-plain-vs-member shape is resolved at plan time.
    DomBind {
        /// The bind directive's local NAME (`value` / `clientWidth` / `open` /
        /// `innerHTML` / …) — the subject the property/dimension/content-editable
        /// helpers pass as a string-literal arg. Carried on the shape so a node with
        /// MULTIPLE binds (`<video bind:currentTime bind:paused …>`) emits each bind's
        /// correct name, never the node's first bind name.
        name: String,
        /// The typed runtime emission routing (the `$.bind_*` / `bind_property`
        /// form, arity, event, prelude) for this `(name, host)`.
        routing: RuntimeBindRouting,
        /// How the bound get/set reach the helper. A TARGET-LVALUE bind synthesizes
        /// the get/set THUNKS from a reassignable lvalue (`() => GET` / `($$value) =>
        /// SET`); a FUNCTION-PAIR bind (`bind:value={get, set}`) passes the two
        /// USER-supplied (and signal-rewritten) get/set expressions DIRECTLY, with no
        /// generated thunk wrapper. The two emit the SAME per-helper argument
        /// structure (which slots carry get vs set), differing only in the wrapper.
        getset: BindGetSetForm,
        /// The `bind:group` accumulator GROUPING key — `Some` ONLY for a `Group` routing,
        /// `None` for every other DOM bind. Two `bind:group` inputs binding the SAME
        /// structural target in the SAME scope carry an EQUAL key (they share one
        /// `binding_group` accumulator); distinct targets carry distinct keys (each gets its
        /// own accumulator). The emitter allocates ONE collision-safe accumulator name per
        /// DISTINCT key in source order (`binding_group`, `binding_group_1`, …) and the
        /// `$.bind_group(<name>, …)` call reads the name back through this key — never a
        /// single component-wide name.
        group_key: Option<GroupBindKey>,
    },
    /// `bind:this` on an intrinsic element — either a bare non-prop IDENTIFIER target
    /// ([`TargetLvalue`](BindGetSetForm::TargetLvalue): `$.bind_this(el, ($$value) => SET,
    /// () => GET)`) or a two-element getter/setter FUNCTION-PAIR `bind:this={get, set}`
    /// ([`FunctionPair`](BindGetSetForm::FunctionPair): the user-supplied get/set passed
    /// DIRECTLY — `$.bind_this(el, set, get)`). The `getset` form discriminates the two
    /// emit shapes, mirroring the `DomBind` get/set wrapper rule. (Component `bind:this`
    /// fails closed upstream at the component-element gate, never reaching here.)
    This {
        /// How the host-instance get/set reach `$.bind_this`: a `TargetLvalue` identifier
        /// target synthesizes the get/set thunks; a `FunctionPair` passes the two
        /// user-supplied (signal-rewritten) expressions directly.
        getset: BindGetSetForm,
    },
}

/// The GROUPING identity of a `bind:group` accumulator — the structural bind TARGET
/// (`keypath`, derived from the typed bind-target fact, never a raw-source compare) plus the
/// lexical `scope`. Mirrors official svelte@5.56.3's `[keypath, bindings]` group identity:
/// two `bind:group` inputs binding the same target in the same scope share ONE accumulator;
/// a different target (or the same spelling in a different scope) gets its own. The emitter
/// maps each distinct key to one allocated `binding_group[_N]` name (in source order).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct GroupBindKey {
    /// The lexical scope the bind target resolves in (so the same spelling in two scopes
    /// stays two groups).
    pub(super) scope: ScopeId,
    /// The structural identifier keypath of the bound target (`"a"`, `"o.x"`, `"g.i.j"`, …),
    /// the [`BindTargetFact::target_keypath`](super::expr::BindTargetFact) — a PURELY
    /// STRUCTURAL key (svelte's operator-/whitespace-insensitive
    /// `extract_all_identifiers_from_expression`), NEVER a raw-source compare and never a
    /// source fallback.
    pub(super) keypath: String,
}

/// How a DOM-value/property bind's getter + setter reach the `$.bind_*` helper.
///
/// A `bind:value={lvalue}` synthesizes the get/set as THUNKS over a reassignable
/// target ([`TargetLvalue`](Self::TargetLvalue)); a `bind:value={get, set}`
/// function-pair passes the two user-supplied expressions DIRECTLY
/// ([`FunctionPair`](Self::FunctionPair)) — official does not re-wrap them. The
/// emitter reads this to decide whether to wrap the plan's getter/setter strings in
/// `() => …` / `($$value) => …` thunks or emit them verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindGetSetForm {
    /// The get/set are synthesized lvalue THUNKS (`() => GET` / `($$value) => SET`)
    /// over a reassignable target.
    TargetLvalue,
    /// The get/set are the two user-supplied expressions of a `{get, set}` pair,
    /// passed DIRECTLY to the helper (already signal-rewritten, no thunk wrapper).
    FunctionPair,
}

/// Classify a supported `bind:` directive into its accepted [`ClientBindShape`], or
/// fail closed.
///
/// `target` is the bind target (`value` / `this`); `tag` is the host element tag;
/// `expr` is the analyzed bound expression (carrying its source AND the shared
/// [`BindTargetFact`](super::expr::BindTargetFact) — the single bind-target authority,
/// computed once at analysis time, so this classifier never re-parses the expression).
/// Both supported binds REQUIRE an explicit bound expression — a sourceless bind
/// (`expr: None`) fails closed, because runtime-op collection emits `$.bind_value` /
/// `$.bind_this` only for an `AttrIr::Bind { expr: Some(_) }`; accepting a sourceless bind
/// would record a shape the emitter then silently drops. The scope-aware binding lookup
/// resolves the target identifier's kind. ONLY a `bind:value` on an `<input>` to a reactive
/// `$state` signal IDENTIFIER and a `bind:this` to a non-prop IDENTIFIER are accepted; a
/// plain-local / prop / member / non-lvalue / sourceless target fails closed (5c).
#[allow(clippy::too_many_arguments)]
pub(super) fn classify_bind_shape(
    target: &str,
    tag: &str,
    host_attrs: &[AttrIr],
    expr: Option<&AnalyzedExpr<'_>>,
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
    //
    // Oracle-verified scoping (svelte@5.56.3): a TS-wrapped bind target is REACHABLE
    // only inside a `<script lang="ts">` component — in a plain `<script>` the
    // official compiler PARSE-REJECTS `bind:value={name!}` (`Expected token }`). A
    // `lang="ts"` component is a settled broad deferral that fails closed ENTIRELY as
    // `TypeScript` at the parse gate, BEFORE any bind classification (characterized by
    // `lang_ts_component_with_bind_targets_fails_closed`). So this TS-wrapped refusal
    // is MOOT for the 5c-reachable surface (a defense-in-depth stop, not a live 5c
    // boundary); the canonical-lvalue-from-TS widening belongs to whenever `lang="ts"`
    // components are opened (a TypeScript-script block), NOT 5c — the underlying
    // TS-spine strip already exists (`expr::expr_wrapped_ident`). Read from the shared
    // bind-target fact (computed once at analysis time) — no per-call reparse.
    //
    // SCOPE: `lvalue_contains_ts` flags a TS-ONLY operator ANYWHERE in the would-be lvalue
    // spine — the spine TOP (`name!` / `name as T` / `(name!)`), a member-OBJECT non-null
    // (`o!.x`), OR a computed-INDEX cast (`a[x as T]` / `a[x!]`). All fail closed here: each
    // is a plain-`<script>` PARSE error in official (`expected_token` / `js_parse_error`,
    // oracle-verified), so Verter must NOT accept-and-strip them to valid JS. The EXACT
    // diagnostic-code parity (`expected_token`/`js_parse_error` vs this structural
    // fail-closed) stays D-26 (the shared `.mjs` template-expression parse authority, which
    // rejects plain-script TS UNIFORMLY across every value position) — this is the structural
    // fail-closed, NOT a bind-only TS code gate. Read from the shared bind-target fact
    // (computed once at analysis time) — no per-call reparse, no source-text scan.
    if let Some(e) = expr {
        if e.bind_target.lvalue_contains_ts {
            return Err(refuse());
        }
    }
    match target {
        // `bind:this` on an INTRINSIC element (a component `bind:this` fails closed
        // upstream at the component-element gate, never reaching here). Two accepted
        // shapes: (1) a DECLARED non-prop IDENTIFIER target (the §1.2-core shape-3
        // `let el;` local) → the identifier `$.bind_this(el, ($$value) => SET, () => GET)`;
        // (2) a two-element getter/setter FUNCTION-PAIR (`bind:this={get, set}`) → the
        // user-supplied get/set passed DIRECTLY (`$.bind_this(el, set, get)`), matching
        // official svelte@5.56.3. A member `bind:this={refs[0]}` / a prop target / a
        // FREE-or-undeclared identifier target is the deferral-ledger member-bind /
        // prop-bind / declared-target-completion form (5c).
        "this" => {
            // The shorthand `bind:this` is not valid Svelte; an explicit target is required.
            let Some(e) = expr else {
                return Err(refuse());
            };
            match e.bind_target.kind {
                Some(BindTargetKind::Identifier) => {
                    // The identifier ROOT comes from the typed bind-target fact
                    // (`root_ident`), NOT `source.trim()` — so a parenthesized identifier
                    // (`bind:this={(el)}`) resolves its root `el`, matching official (which
                    // accepts author parens around a single identifier). An Identifier kind
                    // always carries a root; a missing root fails closed defensively.
                    let Some(name) = e.bind_target.root_ident.as_deref() else {
                        return Err(refuse());
                    };
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
                        _ => Ok(ClientBindShape::This {
                            getset: BindGetSetForm::TargetLvalue,
                        }),
                    }
                }
                // A two-element getter/setter FUNCTION-PAIR `bind:this={get, set}` on an
                // intrinsic element — the user owns the get/set, so there is no lvalue root
                // to validate (mirroring the DOM-value function-pair lane). Acceptance reads
                // the fact's DEFAULT-CLOSED plain-Svelte-JS function-pair slices (parsed
                // `mjs`, exactly two elements, NO TS-only construct); a parenthesized
                // sequence (`bind:this={(get, set)}`) was already rejected upstream as
                // `bind_invalid_parens`. A CLEAN pair passes the supplied get/set DIRECTLY to
                // `$.bind_this` (signal-rewritten at plan time).
                Some(BindTargetKind::FunctionPair) => {
                    if e.bind_target.is_parenthesized_sequence {
                        return Err(refuse());
                    }
                    if e.bind_target.function_pair.is_some() {
                        Ok(ClientBindShape::This {
                            getset: BindGetSetForm::FunctionPair,
                        })
                    } else {
                        Err(refuse())
                    }
                }
                _ => Err(refuse()),
            }
        }
        // Every other bind name routes through the SHARED runtime-bind router (the
        // DATA-DRIVEN authority): `value`/`checked` (builtin form-control binds) +
        // the wide `bind:` family (`group`/media/dimension/contenteditable/property).
        // An unsupported `(name, host)` pair has no routing and fails closed (5c/5f).
        // The host's typed attributes feed the official host-attribute gates.
        name => {
            classify_dom_value_bind(name, tag, host_attrs, expr, scope, bindings, scopes, refuse)
        }
    }
}

/// Classify a DOM value/property bind (`value`/`checked`/`group`/media/dimension/
/// contenteditable/property) on the host `tag`, resolving its runtime routing through
/// the SHARED [`resolve_runtime_bind`] authority and validating the bound target.
///
/// The routing is DATA-DRIVEN — there is NO per-name match arm pile. An accepted bind
/// REQUIRES an explicit bound-expression source (ACCEPTED == EMITTABLE: a sourceless
/// bind would record a shape the emitter drops). The accepted target taxonomy (driven
/// from the typed [`BindTargetKind`] + the scope-aware binding table, NEVER a text
/// scan):
///
/// - a CLEAN (non-TS-wrapped) bare-identifier target whose binding is a reactive
///   `$state` signal (`$.set(name, $$value)`) OR a PLAIN local (`name = $$value`);
/// - a CLEAN member target (`o.x` / `a[i]`) whose ROOT identifier is a reactive
///   `$state` signal (`$.get(o).x = $$value`) OR a PLAIN local (`o.x = $$value`);
/// - a two-element FUNCTION-PAIR `{get, set}`, whose user-supplied get/set are passed
///   directly to the helper (signal-rewritten, no synthesized lvalue thunk).
///
/// A `$props()` / `$bindable` / `$derived` / import root fails closed as a CONSERVATIVE
/// boundary: a `$props()` / `$bindable` write IS a divergent protocol (a `$.prop` flag-7
/// setter), but the IMPORT and `$derived`-member cases fail closed because their
/// correctness depends on import / derived semantics not yet modelled in this vertical —
/// NOT because official uses a divergent accessor (for an import root official emits the
/// identical plain-member form, and a `$derived` member is a plain member write). 5c
/// keeps these fail-closed until that semantics is owned. Object/array `$state`
/// (`BareProxy` / `StateProxy`) roots are not reachable here — the object/array `$state`
/// DECLARATION fails closed upstream at the `$state()` non-primitive-init gate (its
/// lowering is owned by the runes-completion vertical), so only PLAIN-local and
/// `$state`-SIGNAL roots reach this classifier.
#[allow(clippy::too_many_arguments)]
fn classify_dom_value_bind(
    name: &str,
    tag: &str,
    host_attrs: &[AttrIr],
    expr: Option<&AnalyzedExpr<'_>>,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    refuse: impl Fn() -> UnsupportedSvelteRuntimeSurface,
) -> Result<ClientBindShape, UnsupportedSvelteRuntimeSurface> {
    // The runtime routing for this `(name, host)`. No routing ⇒ an unsupported DOM
    // bind (or a component/window-host bind, not yet supported, owned by 5f) ⇒ fail closed.
    let Some(routing) = resolve_runtime_bind(name, tag) else {
        return Err(refuse());
    };
    // The official HOST-ATTRIBUTE gates — a bind that is valid ONLY when its host
    // carries a specific STATIC attribute (svelte@5.56.3 raises a COMPILE ERROR
    // otherwise). The runtime router only sees `(name, tag)`, so without these gates
    // an invalid bind would emit a divergent / runtime-broken module. Driven from the
    // host's typed `AttrIr` inventory (NEVER a source-text scan).
    if !super::host_attr_gate::host_attr_gate_passes(name, tag, &routing, host_attrs) {
        return Err(refuse());
    }
    // ACCEPTED == EMITTABLE: a sourceless bind fails closed (the emitter only emits a
    // bind for an `AttrIr::Bind { expr: Some(_) }`). The shared bind-target fact
    // (computed once at analysis time) is the SOLE classification authority below — no
    // per-call reparse.
    let Some(e) = expr else {
        return Err(refuse());
    };
    let fact = &e.bind_target;
    // The `bind:group` accumulator grouping key — the structural target keypath + scope,
    // computed ONLY for a `Group` routing (every other DOM bind carries `None`). Two inputs
    // binding the SAME structural target in the SAME scope produce an EQUAL key (they share
    // one accumulator); a distinct target produces a distinct key. The keypath is the typed
    // bind-target fact (svelte's operator-/whitespace-insensitive identifier keypath), a
    // PURELY STRUCTURAL key — NEVER a raw-source compare. An accepted `bind:group` target
    // (Identifier/Member) always has a keypath; a target that yields none fails the group
    // bind CLOSED rather than falling back to raw source.
    let group_key = if routing.helper == RuntimeHelper::Group {
        let Some(keypath) = fact.target_keypath.clone() else {
            return Err(refuse());
        };
        Some(GroupBindKey { scope, keypath })
    } else {
        None
    };
    let accept = |getset| {
        Ok(ClientBindShape::DomBind {
            name: name.to_string(),
            routing,
            getset,
            group_key: group_key.clone(),
        })
    };
    match fact.kind {
        // A bare-identifier target: accepted when its binding is a reactive `$state`
        // signal (setter `$.set(name, $$value)`) OR a PLAIN local (setter `name =
        // $$value`). A `$props()` / `$bindable` / `$derived` / import root needs a
        // divergent official protocol and fails closed (the locked-down boundaries).
        Some(BindTargetKind::Identifier) => {
            // The identifier ROOT comes from the typed bind-target fact (`root_ident`), NOT
            // `source.trim()` — so a parenthesized identifier (`bind:value={(v)}`) resolves
            // its root `v` (official accepts author parens around a single identifier). An
            // Identifier kind always carries a root; a missing root fails closed defensively.
            let Some(root_name) = &fact.root_ident else {
                return Err(refuse());
            };
            if bind_root_is_writable_target(bindings, scopes, scope, root_name) {
                accept(BindGetSetForm::TargetLvalue)
            } else {
                Err(refuse())
            }
        }
        // A member target (`o.x` / `a[i]`): accepted when its ROOT identifier is a
        // reactive `$state` signal (`$.get(o).x = $$value`) OR a PLAIN local (`o.x =
        // $$value`). A member rooted at a `$props()` / `$bindable` / `$derived` /
        // import binding is a divergent official surface and fails closed.
        Some(BindTargetKind::Member) => {
            let Some(root_name) = &fact.root_ident else {
                return Err(refuse());
            };
            if bind_root_is_writable_target(bindings, scopes, scope, root_name) {
                accept(BindGetSetForm::TargetLvalue)
            } else {
                Err(refuse())
            }
        }
        // A two-element function-pair `{get, set}`: the user owns the get/set, so there
        // is no lvalue root to validate. Acceptance reads the fact's DEFAULT-CLOSED
        // plain-Svelte-JS function-pair slices (parsed as plain JS `SourceType::mjs()`,
        // mirroring official's Acorn parse, exactly two elements, NO TS-only construct).
        // A TS construct inside either element — `get as any` / `get!` / a typed arrow
        // param, OR a TS-only class/member field, decorator, `implements`,
        // auto-`accessor` — is a plain-`.svelte` PARSE ERROR in official, so the fact's
        // `function_pair` is `None` and it fails closed (the `lang="ts"` widening is a
        // separate surface). A CLEAN pair is accepted; the supplied get/set are passed
        // DIRECTLY to the helper (signal-rewritten through the plain-JS rewrite lane at
        // plan time).
        Some(BindTargetKind::FunctionPair) => {
            // DEFENSIVE fail-CLOSED: an identifier/member-only bind (`bind:group`) NEVER
            // accepts a function-pair (`SequenceExpression`) target — official throws
            // `bind_group_invalid_expression`. The official-reject gate's bind-validation pass
            // (`scan_bind_shape_violations`) already rejects this upstream for BOTH
            // the bare `{get,set}` and the quoted `"{get,set}"` forms; this belt-and-
            // suspenders check means even a future attribute representation that slips
            // past the gate refuses here rather than emitting a wrong `$.bind_group(get,
            // set)`. Data-driven from the contract policy column (no `name == "group"`).
            if matches!(
                bind_target_policy(name, tag),
                BindTargetPolicy::IdentifierOrMemberOnly { .. }
            ) {
                return Err(refuse());
            }
            // DEFENSIVE fail-CLOSED: author PARENTHESES around the sequence
            // (`bind:value={(get, set)}`) are the official `bind_invalid_parens` reject. The
            // official-reject gate's bind-validation pass (`scan_bind_shape_violations`) already
            // rejects this upstream; this belt-and-suspenders check refuses a parenthesized
            // sequence that slips past the gate rather than emitting a wrong
            // `$.bind_value(el, get, set)` (the author parens transparently dropped).
            if fact.is_parenthesized_sequence {
                return Err(refuse());
            }
            if fact.function_pair.is_some() {
                accept(BindGetSetForm::FunctionPair)
            } else {
                Err(refuse())
            }
        }
        // A non-lvalue target (`bind:value={f()}` — a call, a literal, a binary, or a
        // non-two-element sequence) fails closed (5c).
        None => Err(refuse()),
    }
}

/// Whether a bind target's ROOT binding is a WRITABLE target-lvalue root — a reactive
/// `$state` signal (the setter writes via `$.set` / `$.get(obj).x = …`) OR a PLAIN
/// local (the setter assigns directly: `name = $$value` / `o.x = $$value`). These are
/// the two roots whose plain-assignment setter is byte-correct against official.
///
/// A `$props()` / `$bindable` / `$derived` / import root is NOT writable here as a
/// CONSERVATIVE boundary: a `$props()` / `$bindable` write IS a divergent protocol (a
/// `$.prop` flag-7 accessor), but the IMPORT and `$derived`-member cases fail closed
/// because their correctness depends on import / derived semantics not yet modelled in
/// this vertical — NOT because official uses a divergent accessor (an import root emits
/// the identical plain-member form; a `$derived` member is a plain member write). An
/// UNRESOLVED root (no binding row) likewise fails closed (a free / undeclared target is
/// not an emittable lvalue here).
fn bind_root_is_writable_target(
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    scope: ScopeId,
    root_name: &str,
) -> bool {
    matches!(
        bindings.resolve_kind(scopes, scope, root_name),
        Some(k) if is_writable_bind_root(k)
    )
}

/// Whether a binding kind is an ASSIGNMENT-VALID bind root — the ONLY kinds a two-way
/// `bind:` may legally REASSIGN: a `$state` SIGNAL (`$.set(name, $$value)`), a
/// `$.state($.proxy)` reassignable proxy, or a PLAIN local (`name = $$value`). The
/// read-oriented signal kinds — `$derived`, an `{#each}` item, an `{#await}` binding, and a
/// `{@const}` derived — are READABLE but are NOT assignment targets, so they are EXCLUDED.
///
/// This is deliberately NARROWER than the read-shape classifier [`is_signal_binding`]
/// (which admits `Derived` / `EachSignal` / `AwaitSignal` / `LegacyConstDerived` for
/// interpolation/runtime READ decisions): a signal being READABLE does not make it a valid
/// bind WRITE target. The write decision ([`bind_root_is_writable_target`]) consults this
/// predicate; the read decisions keep [`is_signal_binding`].
fn is_writable_bind_root(kind: BindingRuntimeKind) -> bool {
    matches!(
        kind,
        BindingRuntimeKind::StateSignal { .. }
            | BindingRuntimeKind::StateProxy
            | BindingRuntimeKind::PlainLocal
    )
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

    /// Build a real [`AnalyzedExpr`] for `source` through the SAME single-parse analysis
    /// path the runtime uses (so the test exercises the actual shared `BindTargetFact`,
    /// not a synthetic stand-in).
    fn analyzed_expr(source: &'static str, scope: ScopeId) -> AnalyzedExpr<'static> {
        let facts = crate::svelte::runtime::expr::collect_expr_references(source)
            .expect("test bind expression parses cleanly");
        AnalyzedExpr::interned(source, scope, facts)
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
            "value",
            "input",
            /* host_attrs = */ &[],
            /* expr = */ None,
            root,
            &bindings,
            &scopes,
            &locals,
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
        let value_expr = analyzed_expr("value", root);
        let res = classify_bind_shape(
            "value",
            "input",
            /* host_attrs = */ &[],
            /* expr = */ Some(&value_expr),
            root,
            &bindings,
            &scopes,
            &locals,
            span,
        );
        // An explicit `bind:value={value}` to a $state signal is accepted as a DOM
        // value bind carrying the `$.bind_value` routing.
        match res {
            Ok(ClientBindShape::DomBind {
                name,
                routing,
                getset,
                group_key,
            }) => {
                assert_eq!(name, "value");
                assert_eq!(
                    routing.helper,
                    crate::svelte::bind_contract::RuntimeHelper::Value
                );
                // A bare-identifier signal target synthesizes the lvalue thunks.
                assert_eq!(getset, BindGetSetForm::TargetLvalue);
                // NEGATIVE: a non-`group` DOM bind carries NO group key (the accumulator
                // grouping is `bind:group`-only).
                assert_eq!(group_key, None, "a bind:value carries no group key");
            }
            other => panic!("expected a DomBind(Value) shape, got {other:?}"),
        }
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
        let el_expr = analyzed_expr("el", root);
        let declared = classify_bind_shape(
            "this",
            "div",
            /* host_attrs = */ &[],
            Some(&el_expr),
            root,
            &bindings,
            &scopes,
            &locals,
            span,
        );
        assert_eq!(
            declared,
            Ok(ClientBindShape::This {
                getset: BindGetSetForm::TargetLvalue
            }),
            "a declared `let el;` bind:this target is the supported identifier shape-3"
        );

        // FREE target `button` (undeclared) — fails closed.
        let button_expr = analyzed_expr("button", root);
        let free = classify_bind_shape(
            "this",
            "button",
            /* host_attrs = */ &[],
            Some(&button_expr),
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

    /// A scope graph holding ONE binding of the given `kind` named `root` — so the
    /// writability decision can be exercised for each binding-runtime kind.
    fn env_with_root_kind(kind: BindingRuntimeKind) -> (BindingTable, ScopeGraph, ScopeId) {
        let (mut graph, root) = ScopeGraph::with_root();
        let mut bindings = BindingTable::new();
        let id = bindings.push(BindingInfo {
            name: "root".to_string(),
            scope: root,
            kind,
            state: None,
        });
        graph.declare(root, "root", id);
        (bindings, graph, root)
    }

    #[test]
    fn bind_root_writability_admits_only_assignment_valid_kinds() {
        // The WRITE decision (`bind_root_is_writable_target`) must admit ONLY the
        // assignment-valid roots — a `$state` SIGNAL, a `$.state($.proxy)` reassignable
        // proxy, and a PLAIN local — and must EXCLUDE the read-oriented signal kinds a
        // bind cannot legally reassign: `$derived`, an `{#each}` item, an `{#await}`
        // binding, and a `{@const}` derived. RED before the fix: the write decision reused
        // the read-oriented `is_signal_binding`, which admits `Derived` / `EachSignal` /
        // `AwaitSignal` / `LegacyConstDerived` — so a read-only root was wrongly treated as
        // writable.
        for kind in [
            BindingRuntimeKind::StateSignal { raw: false },
            BindingRuntimeKind::StateSignal { raw: true },
            BindingRuntimeKind::StateProxy,
            BindingRuntimeKind::PlainLocal,
        ] {
            let (bindings, scopes, root) = env_with_root_kind(kind);
            assert!(
                bind_root_is_writable_target(&bindings, &scopes, root, "root"),
                "an assignment-valid root ({kind:?}) must be writable"
            );
        }
        for kind in [
            BindingRuntimeKind::Derived,
            BindingRuntimeKind::EachSignal,
            BindingRuntimeKind::AwaitSignal,
            BindingRuntimeKind::LegacyConstDerived,
        ] {
            let (bindings, scopes, root) = env_with_root_kind(kind);
            assert!(
                !bind_root_is_writable_target(&bindings, &scopes, root, "root"),
                "a read-only signal root ({kind:?}) must NOT be writable (no bind reassignment)"
            );
        }
    }

    #[test]
    fn is_writable_bind_root_admits_only_assignment_valid_kinds() {
        // The writable predicate admits EXACTLY the assignment-valid kinds (a `$state`
        // signal, a reassignable proxy, a plain local) and EXCLUDES the read-oriented signal
        // kinds the read classifier (`is_signal_binding`) admits — `Derived` / `EachSignal` /
        // `AwaitSignal` / `LegacyConstDerived`. A signal being READABLE does not make it a
        // valid bind WRITE target.
        assert!(is_writable_bind_root(BindingRuntimeKind::StateSignal {
            raw: false
        }));
        assert!(is_writable_bind_root(BindingRuntimeKind::StateSignal {
            raw: true
        }));
        assert!(is_writable_bind_root(BindingRuntimeKind::StateProxy));
        assert!(is_writable_bind_root(BindingRuntimeKind::PlainLocal));
        assert!(!is_writable_bind_root(BindingRuntimeKind::Derived));
        assert!(!is_writable_bind_root(BindingRuntimeKind::EachSignal));
        assert!(!is_writable_bind_root(BindingRuntimeKind::AwaitSignal));
        assert!(!is_writable_bind_root(
            BindingRuntimeKind::LegacyConstDerived
        ));
        // Read-only signal kinds the read classifier admits are NOT writable — the explicit
        // split this predicate enforces.
        assert!(is_signal_binding(BindingRuntimeKind::Derived));
        assert!(!is_writable_bind_root(BindingRuntimeKind::Derived));
    }
}
