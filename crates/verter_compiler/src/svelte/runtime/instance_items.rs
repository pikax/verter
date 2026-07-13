//! The strict finite instance-script item allowlist (the supported top-level
//! `<script>` declaration shapes) for the default-deny Svelte client surface.
//!
//! This is the script-side analogue of the element allowlist: the classifier
//! ([`classify_supported_instance_items`]) admits ONLY the enumerated
//! [`SupportedInstanceScriptItem`] shapes and the lowering consumes ONLY this enum —
//! there is NO "emit any non-rune statement" path. (A named `function` declaration is
//! admitted ONLY when its name is referenced by an accepted DOM function-pair bind; its
//! body lowers through the shared rewriter, not verbatim.) It also owns the scope-aware
//! magic-identifier scan ([`scan_magic_identifiers`]) that refuses a reference to a
//! compiler-magic object (`$$slots` / `$$props` / `$$restProps`).
//!
//! Every decision is driven from the typed OXC AST (statement kind, declarator
//! pattern, init shape, TS-annotation presence) + the scope-aware binding table,
//! never a raw text scan.

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Comment, Expression, Statement, VariableDeclarationKind};
use oxc_span::GetSpan;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::expr::{
    call_internal_comment_trivia, carrier_tail_comment_trivia, is_derived_callee, is_props_callee,
    reparse_module, state_rune_call,
};
use verter_span::Span;
// The declarator-shape probes (`$effect.root`/`$effect.tracking` init +
// `$props.id()` declarator) live in the sibling `instance_item_shapes` module;
// re-exported so the state scan keeps the `instance_items::` path.
pub(super) use super::instance_item_shapes::{
    classify_effect_rune_init, classify_props_id_decl, effect_rune_init_shape, props_id_decl_shape,
};

// ---------------------------------------------------------------------------
// Instance-script item allowlist (the strict finite supported-shape set)
// ---------------------------------------------------------------------------

/// A TYPED supported instance-script item — the closed allowlist of top-level
/// instance-script declaration shapes the client core lowers. This is the
/// script-side analogue of [`SupportedHtmlElement`](super::client_allowlist::SupportedHtmlElement):
/// the classifier ([`classify_supported_instance_items`]) admits ONLY these enumerated
/// shapes and the lowering consumes ONLY this enum — there is NO "emit any non-rune
/// statement" path. Every OTHER top-level item (a function NOT referenced by a DOM
/// function-pair bind / class / enum / namespace / interface / type / plain non-rune
/// `let`-`const`-`var` / arbitrary statement / `$:` label / `$`-`$$`-prefixed binding)
/// fails closed BY CONSTRUCTION at the classifier.
///
/// Each variant carries the lowering inputs (a binding name, the init payload text, or
/// the function source text), so the lowering is a thin per-variant transform — a
/// declaration's init / a function body lowers through the shared rewriter, never a
/// re-walk of an arbitrary statement.
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
    /// A single `$props()` destructure (`let { a } = $props()` / `let { a: b } =
    /// $props()` / `let { a = 1, v = $bindable(0) } = $props()`). The destructure
    /// emits no VERBATIM declaration: a no-default member is read directly off
    /// `$$props`, and a default-bearing / written member lowers to a `$.prop(...)`
    /// prop-source declaration projected from the props-shape facts — so this
    /// variant carries no lowering payload; it is the classification fact that the
    /// props destructure was accepted.
    PropsDestructure,
    /// `let el;` — a bare (no-init, no-annotation) `let` identifier used SOLELY as a
    /// supported `bind:this` target. Carries the binding name (lowered to `let el;`).
    BindThisLocal {
        /// The declared local name.
        name: String,
    },
    /// `let v = <literal-only init>;` / `let v;` — a PLAIN local (no rune call, no TS
    /// annotation) used SOLELY as a DOM bind TARGET (a `bind:value={v}` ident or the
    /// ROOT of a `bind:value={v.x}` member). Official emits the declaration VERBATIM (it
    /// stays a plain local — `let v = "x";` / `let o = { x: '' };` / `let v;`), so this
    /// variant carries the binding name + the LITERAL-ONLY init source (a string/number/
    /// bool/null/bigint literal, or an object/array literal whose values are recursively
    /// literal), emitted byte-for-byte — or `None` for an UNINITIALIZED `let v;` (the
    /// no-init plain-local form, lowered to a bare `let v;`). The init is restricted to a
    /// literal-only value precisely so the verbatim emit is correct without an init
    /// rewrite — a signal-bearing / identifier-bearing init (which official would
    /// `$.get`-rewrite) is a DISTINCT surface that fails closed.
    BindLocalLet {
        /// The declared local name.
        name: String,
        /// The literal-only init source text (emitted verbatim), or `None` for an
        /// uninitialized `let v;`.
        init: Option<String>,
    },
    /// A named top-level `function name(...) { ... }` declaration whose name is EXACTLY
    /// referenced by an accepted DOM function-pair bind (`bind:value={get, set}`).
    /// Official emits the declaration with its BODY signal reads/writes rewritten
    /// (`function get() { return $.get(value); }`), then passes the function ident
    /// directly to the helper. This variant carries the function's full source text; the
    /// body lowers through the shared FALLIBLE expression rewriter at projection time (so
    /// a signal read/write inside the body becomes `$.get`/`$.set`), NOT verbatim. The
    /// admission is gated on the function-pair-referenced name set — a function NOT
    /// referenced by such a bind is out-of-allowlist and fails closed.
    FunctionDecl {
        /// The declared function name (the function-pair-referenced ident).
        name: String,
        /// The function declaration's full source text (lowered via the rewriter).
        source: String,
    },
    /// A top-level `$inspect(...);` / `$inspect(...).with(...);` expression
    /// STATEMENT — the production-ELIDED `$inspect` family. Official
    /// `svelte@5.56.3` (`dev:false`) removes the whole statement (leaving only a
    /// cosmetic `;;` empty-statement residue); Verter lowers it to NOTHING (no
    /// client-body item, no helper, no import, no dev form), so the variant
    /// carries no payload. A `.with(...)` chain still FORCES the component
    /// context frame (`$.push($$props, true)` / `$.pop()` + the `$$props`
    /// param) — that fact is owned by the [`super::reactive_analysis`]
    /// `needs_context` scan, not by this item (which records only the elision).
    InspectElided,
    /// A top-level effect-family expression STATEMENT — a WELL-FORMED
    /// `$effect(fn);` / `$effect.pre(fn);` user-effect call (the user-effect
    /// members' ONLY official-legal position) OR an UNASSIGNED bare
    /// `$effect.root(fn);` / `$effect.tracking();` (official accepts the
    /// unassigned side-effect forms — no frame). The shared
    /// [`effect_family_call_fact`](super::expr::effect_family_call_fact)
    /// classifier proves the form. Carries the call-EXPRESSION source; the whole
    /// expression lowers through the shared FALLIBLE rewriter at projection time
    /// in the STATEMENT role (the callee → its registered helper, body signal
    /// reads → `$.get`, an `await` inside the callback refuses as
    /// experimental-async) — NEVER a string synthesis. The `$.push`/`$.pop`
    /// frame fact is owned by the [`super::reactive_analysis`] `needs_context`
    /// scan (only the user-effect members force it).
    EffectStatement {
        /// The effect-family call-expression source text.
        source: String,
        /// The pre-rendered CALL-INTERNAL trivia of the comments the carrier
        /// slice would otherwise silently drop — the transparent-WRAPPER head
        /// range between the statement-expression start and the call start
        /// (`(/*#__PURE__*/ $effect(fn));`: the wrapper parens stay outside
        /// the carried call span, but their interior head comments must
        /// survive). The rewriter re-emits it INSIDE the emitted helper call,
        /// immediately after the opening paren — the canonical call-internal
        /// slot, never call-leading — ahead of the head's own trivia (source
        /// order). Empty for the wrapper-less common case.
        head_trivia: String,
        /// The pre-rendered trivia of the carrier's LEXICAL TAIL
        /// ([`carrier_tail_comment_trivia`](super::expr::carrier_tail_comment_trivia)):
        /// the statement span's interior after the call end — a normalized
        /// wrapper's interior (`($effect.root(fn) /*!license*/);`) and an
        /// unwrapped call's pre-`;` trailing comments (`$effect.root(fn)
        /// /*!license*/;`) — PLUS the same-line ASI extension of a
        /// semicolon-less statement (`$effect.root(fn) /*!license*/`): a
        /// license-class trailing comment stays in contract with or without
        /// the explicit `;`. The projection re-emits it AFTER the rewritten
        /// call payload, before the generated `;` (line comments keep their
        /// newline terminator, so the `;` can never be commented out). Empty
        /// for the trailing-comment-free common case.
        tail_trivia: String,
    },
    /// A top-level `let`/`const` declaration containing EXACTLY ONE declarator
    /// whose init is a WELL-FORMED `$props.id()` call (zero args, no spread, no
    /// optional chaining), plus zero or more LITERAL-ONLY sibling declarators.
    /// Official hoists the id declarator to the FUNCTION-BODY TOP as
    /// `const <name> = $.props_id();` (the source keyword is NOT preserved — a
    /// `let` still emits `const`) and splits the siblings into their own
    /// declaration at the statement's source slot, keyword preserved. The
    /// lowering emits the hoist through the plan's dedicated body-top slot and
    /// the siblings as an ordinary body statement. The shared
    /// [`props_id_decl_shape`] predicate proves the shape; a declaration it
    /// refuses (a `var`, a TS-annotated declarator, a non-literal sibling init, a
    /// second `$props.id()` declarator) falls through to the existing fail-closed
    /// gates.
    PropsIdDecl {
        /// The declared id binding name (hoisted as `const <name> = $.props_id();`).
        name: String,
        /// Whether the declaration keyword is `const` (else `let`) — preserved on
        /// the SIBLING declaration only (the hoisted id decl is always `const`).
        const_decl: bool,
        /// The literal-only sibling declarators, in source order, as
        /// `(name, init source text)` rows (`None` init = a bare `let a;`).
        siblings: Vec<(String, Option<String>)>,
    },
    /// A top-level single-declarator `const NAME = <init>;` admitted as a
    /// `$store` SOURCE: either `NAME` is the base of a classified `$NAME`
    /// subscription (`const count = writable(0)` admitted because `$count`
    /// subscribes) or the declaration is a store DEPENDENCY reachable from a
    /// subscribed base through the demand-driven admission closure (`const
    /// doubled = derived(a, …)` with `{$doubled}` admits `a`). NO
    /// import-source/type gate — a hand-rolled local factory's const is the
    /// same carrier. Carries the binding name + init source; the init lowers
    /// through the shared FALLIBLE rewriter at projection time (a shadowed
    /// `$a` callback param stays verbatim; a store read/write inside the init
    /// rewrites). An arbitrary call-initialized const with NO subscription
    /// stays out of this carrier and fails closed at the const gate below.
    StoreSourceDecl {
        /// The declared store-source binding name.
        name: String,
        /// The declarator INIT expression source text (rewriter-lowered).
        init: String,
    },
    /// A top-level `class NAME { … }` declaration admitted as a `$store`
    /// DEPENDENCY: `NAME` is reachable from a subscribed base through the
    /// demand-driven admission closure (`const c = new S(); {$c}` admits the
    /// store class `S`). Official emits the class body VERBATIM (frames the
    /// store via `new`), so this variant carries the class's full source text
    /// and lowers byte-for-byte — NO rewriter pass (a class body is plain JS,
    /// not a signal-bearing reactive surface). A class NOT reachable from any
    /// classified subscription is out-of-allowlist and fails closed (out of the
    /// store-subscription scope).
    StoreClassDecl {
        /// The declared class name (the store-dependency-closure referent).
        name: String,
        /// The class declaration's full source text (emitted verbatim).
        source: String,
    },
    /// A top-level single-declarator `const`/`let` whose init is a WELL-FORMED
    /// zero-argument call of an ADMITTED `createEventDispatcher` import local
    /// (`const dispatch = createEventDispatcher();`) — the component-event
    /// dispatcher surface. Official emits the declaration VERBATIM (the
    /// dispatcher and its `dispatch(...)` calls stay PLAIN calls — never a
    /// runtime-helper rewrite); the imported call independently sets the shared
    /// `needs_context` fact, which supplies the `$.push`/`$.init`/`$.pop` frame
    /// under legacy mode. Mode-independent (a runes dispatcher emits the same
    /// declaration under the runes frame). The admission is gated on the
    /// dispatcher-local set (the `svelte`-module import whose IMPORTED name is
    /// `createEventDispatcher`) — an arbitrary call-initialized const keeps its
    /// fail-closed refusal.
    DispatcherDecl {
        /// Whether the declaration keyword is `const` (else `let`) — preserved.
        const_decl: bool,
        /// The declared dispatcher binding name.
        name: String,
        /// The imported callee LOCAL name (`createEventDispatcher` or its alias).
        callee: String,
    },
    /// A LEGACY `export let` prop statement — the legacy PROP surface. Carries
    /// the statement's declared LOCAL names in source order; each lowers to its
    /// own `let <local> = $.prop($$props, '<key>', <flags>[, <default>]);`
    /// declaration through the SHARED prop-source substrate (the unified
    /// declarator plan owns the member facts + default lowering — legacy base
    /// flags 8, `UPDATED` +4 for a written prop, the same official simple/lazy
    /// default algorithm). Minted ONLY under LEGACY mode: a runes-mode `export
    /// let` is the official `legacy_export_invalid` compile error.
    ExportLetProps {
        /// The exported prop LOCAL names, in source order.
        locals: Vec<String>,
    },
    /// A LEGACY top-level plain `let` PROMOTED to a `$.mutable_source` signal —
    /// the demand-driven legacy reactivity (the binding is WRITTEN: reassigned,
    /// member-mutated, or a `bind:` target). Lowers to `let <name> =
    /// $.mutable_source(<rewritten init>);` (zero-arg for the uninitialized
    /// form); the init lowers through the shared FALLIBLE rewriter, and every
    /// read/write of the binding routes through the shared signal rewriter
    /// (`$.get` / `$.set` / `$.update` / member `$.mutate`). An UNWRITTEN plain
    /// `let` never mints this item (it keeps its fail-closed refusal).
    MutableSourceLet {
        /// The declared signal name.
        name: String,
        /// The init expression source text (rewriter-lowered), or `None` for an
        /// uninitialized `let v;` (the zero-arg `$.mutable_source()` form).
        init: Option<String>,
    },
    /// A top-level `let`/`const` declarator whose init is a WELL-FORMED
    /// assignable effect-family EXPRESSION rune — `$effect.root(fn)` (the result
    /// is the teardown function) or `$effect.tracking()`. Carries the declaration
    /// keyword, the binding name, and the INIT expression source; the init lowers
    /// through the shared FALLIBLE rewriter at projection time (the callee →
    /// `$.effect_root` / `$.effect_tracking`, nested effects and signal reads
    /// rewrite recursively). A `var` declarator stays out of the carrier (the
    /// official `var` read semantics are a distinct surface).
    EffectRuneInit {
        /// Whether the declaration is a `const` (else `let`) — preserved verbatim.
        const_decl: bool,
        /// The declared binding name.
        name: String,
        /// The declarator INIT expression source text.
        init: String,
        /// The pre-rendered CALL-INTERNAL trivia of the transparent-WRAPPER
        /// head comments the init slice would otherwise silently drop
        /// (`const stop = (/*#__PURE__*/ $effect.root(fn));`) — re-emitted by
        /// the rewriter inside the emitted helper call, immediately after the
        /// opening paren (never call-leading). Empty for the wrapper-less
        /// common case.
        head_trivia: String,
        /// The pre-rendered trivia of the carrier's LEXICAL TAIL
        /// ([`carrier_tail_comment_trivia`](super::expr::carrier_tail_comment_trivia)):
        /// the declaration span's interior after the call end — a normalized
        /// wrapper's interior (`const s = ($effect.root(fn) /*!license*/);`)
        /// and an unwrapped init's pre-`;` trailing comments (`const s =
        /// $effect.root(fn) /*!license*/;`) — PLUS the same-line ASI extension
        /// of a semicolon-less declaration (`const s = $effect.root(fn)
        /// /*!license*/`). The projection re-emits it AFTER the rewritten call
        /// payload, before the generated `;`. Empty for the
        /// trailing-comment-free common case.
        tail_trivia: String,
    },
    /// A LEGACY `$:` reactive statement — lowers to ONE
    /// `$.legacy_pre_effect(<deps>, <body>)` registration (the registrations
    /// emit AFTER every other body statement, in DEPENDENCY order, followed by
    /// the single `$.legacy_pre_effect_reset()`). The BODY wraps as the effect
    /// thunk through the shared FALLIBLE rewriter; the dependency thunk wraps
    /// each dependency read by its resolved binding kind (`$.get(x)` for a
    /// mutable-source local, `$.deep_read_state(p())` for a legacy prop, the
    /// bare accessor call `$c()` for a store subscription, the bare name for an
    /// import). An IMPLICIT bare-identifier assignment target was already
    /// declared as a `$.mutable_source` binding by the analysis pass, so the
    /// body assignment rewrites through the shared signal rewriter
    /// (`$.set(y, …)`) with no per-shape lowering fork. Minted ONLY under
    /// LEGACY mode: a runes-mode `$:` is the official
    /// `legacy_reactive_statement_invalid` compile error.
    ReactiveStatement {
        /// The typed BODY shape (the effect-thunk payload).
        body: super::legacy_reactive::ReactiveStatementBody,
        /// Dependency-candidate names in first-mention order (shadow-pruned;
        /// pure `=`-assignment-target-only names excluded). Kind resolution
        /// and the plain-local/global exclusion happen at lowering.
        deps: Vec<String>,
        /// The assigned names (the official assignment/update extraction) —
        /// the registration-order and cycle-detection input.
        assignments: Vec<String>,
        /// The labeled statement's source span (the cycle-reject span).
        span: Span,
    },
}

/// Classify the instance script's TOP-LEVEL items into the strict finite
/// [`SupportedInstanceScriptItem`] allowlist, or fail closed on the FIRST
/// out-of-allowlist item.
///
/// The seven supported shapes are EXACTLY:
/// 1. `let name = $state(<primitive literal>);`
/// 2. a single `$props()` destructure (plain and `$bindable(...)` defaults
///    included — the member shape is enforced by the props-shape gate);
/// 3. `let el;` used solely as a supported `bind:this` target;
/// 4. `let v = <literal-only init>;` used solely as a DOM bind-TARGET lvalue root;
/// 5. a production-ELIDED `$inspect(...);` / `$inspect(...).with(...);` statement
///    (lowered to nothing);
/// 6. a well-formed effect-family statement — `$effect(fn);` / `$effect.pre(fn);`
///    or an unassigned bare `$effect.root(fn);` / `$effect.tracking();` (lowered
///    through the shared rewriter);
/// 7. a `let`/`const` declarator whose init is a well-formed `$effect.root(fn)` /
///    `$effect.tracking()` expression rune (lowered through the shared rewriter).
///
/// `bind_this_targets` is the set of local names used as a supported `bind:this`
/// target (from the accepted bind shapes) — a bare `let el;` is admitted ONLY when
/// its name is in this set; an unused / plain bare local fails closed.
///
/// `bind_lvalue_roots` is the set of plain-local names used as a DOM bind-target
/// lvalue ROOT (a `bind:value={v}` ident or the root of a `bind:value={v.x}` member)
/// — a `let v = <literal init>;` / `let v;` is admitted as the plain-local bind shape
/// ONLY when its name is in this set; an unused / non-target plain local fails closed.
///
/// `bind_function_pair_names` is the set of bare-identifier names referenced by an
/// accepted DOM FUNCTION-PAIR bind (`bind:value={get, set}`) — a top-level
/// `function name(...) {}` declaration is admitted ONLY when its name is in this set
/// (its body lowers via the shared rewriter at projection time). A function NOT
/// referenced by such a bind is out-of-allowlist and fails closed (there is NO wildcard
/// "emit any function" path).
///
/// Everything else fails closed: a plain `let x = 0` NOT used as a bind target, a
/// `const` / `var`, a top-level function NOT referenced by a bind pair / class / enum /
/// namespace / interface / type, an arbitrary expression / control-flow / empty
/// statement, a `$:` reactive label, an import / export, a `$` / `$$`-prefixed binding,
/// and the magic refs `$$slots` / `$$props` / `$$restProps`. The decision is driven from
/// the typed OXC AST (statement kind, declarator pattern, init shape, TS-annotation
/// presence), never a text scan.
///
/// Two whole-program pre-passes run FIRST so their PRECISE diagnostics win over the
/// generic item refusal: the rune-form / rune-position scan (owned by
/// [`super::client_surface`]) and the magic-identifier scan ([`scan_magic_identifiers`]).
pub(super) fn classify_supported_instance_items(
    instance_source: &str,
    bind_this_targets: &[String],
    bind_lvalue_roots: &[String],
    bind_function_pair_names: &[String],
    store_admissions: &StoreScriptAdmissions,
    legacy: &LegacyScriptFacts,
    dispatcher_locals: &rustc_hash::FxHashSet<String>,
) -> Result<Vec<SupportedInstanceScriptItem>, UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance_source) else {
        // An unparseable instance script is recorded as a script-parse diagnostic
        // upstream; classify yields no items (the upstream parse gate owns the refusal).
        return Ok(Vec::new());
    };

    let mut items = Vec::new();
    for stmt in &program.body {
        // An `import` reaching here is an ADMITTED static import (every static form —
        // default / named / namespace / side-effect / mixed — classifies BEFORE this
        // allowlist; only the non-static residual fails closed there). It is HOISTED
        // to module scope via the `UserImport` carrier — NOT a component-function body
        // statement — so it contributes no `SupportedInstanceScriptItem`.
        if matches!(stmt, Statement::ImportDeclaration(_)) {
            continue;
        }
        items.push(classify_instance_statement(
            stmt,
            instance_source,
            &program.comments,
            bind_this_targets,
            bind_lvalue_roots,
            bind_function_pair_names,
            store_admissions,
            legacy,
            dispatcher_locals,
        )?);
    }
    Ok(items)
}

/// The LEGACY-mode script facts the item classifier consumes: whether the FINAL
/// lowered mode is legacy (the `export let` prop accept and the `let` promotion
/// are LEGACY-ONLY — the accept sites themselves are mode-gated, so a runes-mode
/// `export let` can never take legacy lowering even if an upstream gate were
/// bypassed), plus the PROMOTED `let` names (the root-scope bindings the
/// demand-driven legacy promotion flipped to `$.mutable_source` — read from the
/// finalized binding table by the caller, never re-derived here).
#[derive(Debug, Default)]
pub(super) struct LegacyScriptFacts {
    /// Whether the component's FINAL mode is `SvelteMode::Legacy`.
    pub(super) legacy_mode: bool,
    /// The promoted top-level `let` names (kind == `MutableSource`).
    pub(super) promoted_lets: rustc_hash::FxHashSet<String>,
}

/// The DEMAND-DRIVEN `$store` script admissions the item classifier consumes —
/// computed by the classifier's caller from the classified subscriptions (the
/// [`store_dependency_closure`](super::store_subscriptions::store_dependency_closure)
/// seeded by subscribed bases, plus the bare-identifier event-handler function
/// referents). Empty for a component with no `$name` subscription — nothing is
/// admitted store-wise, so an arbitrary call-initialized const / unreferenced
/// function keeps its existing fail-closed refusal.
#[derive(Debug, Default)]
pub(super) struct StoreScriptAdmissions {
    /// The top-level `const` names admitted as store sources / dependencies.
    pub(super) const_names: rustc_hash::FxHashSet<String>,
    /// The top-level `class` names admitted as store dependencies — a local
    /// store CLASS reached transitively from a subscribed base (`const c = new
    /// S()` admits `S`), lowered verbatim. A class NOT reached from any
    /// subscription stays out of this set and fails closed (out of the
    /// store-subscription scope).
    pub(super) class_names: rustc_hash::FxHashSet<String>,
    /// The top-level `function` names admitted beyond the function-pair set: the
    /// store dependency-closure functions (a local store factory) plus the
    /// bare-identifier event-handler referents (`onclick={inc}`).
    pub(super) function_names: rustc_hash::FxHashSet<String>,
}

/// Classify ONE top-level instance-script statement into its supported item, or
/// fail closed. The supported statements are EXACTLY a variable declaration
/// matching a `$state` / `$props()` / `bind:this` / plain-local bind /
/// effect-rune-init shape, a named `function` declaration referenced by a DOM
/// function-pair bind, a production-elided `$inspect(...);` /
/// `$inspect(...).with(...);` statement, OR a well-formed `$effect(fn);` /
/// `$effect.pre(fn);` user-effect statement; every other statement kind fails
/// closed with a precise `construct` label. `comments` is the instance parse's
/// comment table — the effect-family carriers consult it so a transparent
/// wrapper's head trivia rides the carrier instead of being sliced away.
#[allow(clippy::too_many_arguments)]
fn classify_instance_statement(
    stmt: &Statement<'_>,
    instance_source: &str,
    comments: &[Comment],
    bind_this_targets: &[String],
    bind_lvalue_roots: &[String],
    bind_function_pair_names: &[String],
    store_admissions: &StoreScriptAdmissions,
    legacy: &LegacyScriptFacts,
    dispatcher_locals: &rustc_hash::FxHashSet<String>,
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    match stmt {
        Statement::VariableDeclaration(decl) => classify_instance_variable_decl(
            decl,
            instance_source,
            comments,
            bind_this_targets,
            bind_lvalue_roots,
            store_admissions,
            legacy,
            dispatcher_locals,
        ),
        // The export-family split. `export let <ident>…` is the LEGACY prop
        // surface (accepted per-declarator, LEGACY mode only — the accept site
        // itself is mode-gated); `export const` / `export function` / `export
        // class` are the `$$exports` component-export surface with their OWN
        // fail-closed identity (any mode); every other export form keeps its
        // precise fail-closed residual.
        Statement::ExportNamedDeclaration(export) => classify_export_statement(export, legacy),
        // A named top-level `function name(...) {}` is admitted ONLY when its name is
        // EXACTLY referenced by an accepted DOM function-pair bind (`bind:value={get,
        // set}`), by a bare-identifier event handler (`onclick={inc}`), or by the
        // `$store` dependency closure (a local store factory); its body lowers via
        // the shared rewriter at projection time. A function referenced by NONE of
        // those, or an ANONYMOUS function declaration (no usable name to bind), is
        // out-of-allowlist and fails closed.
        Statement::FunctionDeclaration(func) => classify_function_declaration(
            func,
            instance_source,
            bind_function_pair_names,
            store_admissions,
        ),
        // A named top-level `class NAME { … }` is admitted ONLY when `NAME` is in
        // the `$store` dependency closure (a local store CLASS reached from a
        // subscribed base, `const c = new S(); {$c}`); its body emits VERBATIM.
        // A class referenced by NO subscription (or an anonymous class with no
        // usable name) is out-of-allowlist and fails closed (out of the
        // store-subscription scope).
        Statement::ClassDeclaration(class) => {
            classify_class_declaration(class, instance_source, store_admissions)
        }
        // A top-level `$inspect(...);` / `$inspect(...).with(...);` expression
        // statement is production-ELIDED (official `dev:false` removes the whole
        // statement). Classified from the TYPED OXC expression shape; every OTHER
        // expression statement — including a top-level `$inspect.trace();`, an
        // official ERROR (`inspect_trace_invalid_placement`) — still fails closed
        // below. (A top-level shadowing `let $inspect` is a `$`-prefixed binding
        // refused by the declarator gate, so the bare name here is the rune.)
        Statement::ExpressionStatement(stmt) if is_inspect_elision_expression(&stmt.expression) => {
            Ok(SupportedInstanceScriptItem::InspectElided)
        }
        // A top-level effect-family expression statement — a WELL-FORMED
        // `$effect(fn);` / `$effect.pre(fn);` user-effect call OR an unassigned
        // bare `$effect.root(fn);` / `$effect.tracking();` — admitted as the
        // rewriter-backed effect-statement carrier. Classified from the TYPED
        // OXC call shape via the shared family classifier; a malformed /
        // uncalled statement form falls through to the generic refusal (the
        // rune scan already failed the malformed forms closed upstream). (A
        // top-level shadowing `let $effect` is a `$`-prefixed binding refused
        // by the declarator gate, so the bare name here is the rune.)
        Statement::ExpressionStatement(stmt) => {
            if let Some(item) = classify_effect_statement(
                &stmt.expression,
                stmt.span.end,
                instance_source,
                comments,
            ) {
                Ok(item)
            } else {
                Err(UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
                    construct: "expression statement",
                    span: Span::new(stmt.span.start, stmt.span.end),
                })
            }
        }
        // A `$:` labeled statement — the LEGACY reactive-statement surface. The
        // accept site is mode-gated exactly like the `export let` arm: under
        // legacy mode it mints the typed reactive-statement item (body shape +
        // dependency/assignment facts from the typed AST); under RUNES mode the
        // SAME statement is the official `legacy_reactive_statement_invalid`
        // compile error (the pre-lowering gate and the classifier's runes-side
        // twin normally reject first; this arm keeps the accept itself
        // airtight). A non-`$` label stays the generic fail-closed residual.
        Statement::LabeledStatement(labeled) if labeled.label.name == "$" => {
            let span = Span::new(stmt.span().start, stmt.span().end);
            if !legacy.legacy_mode {
                return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                    rejection: super::official_rule::OfficialRejection::of(
                        super::official_rule::CoreOfficialValidationRule::LegacyReactiveStatementInvalid,
                    ),
                    span,
                });
            }
            let facts = super::legacy_reactive::reactive_statement_facts(&labeled.body);
            let body = super::legacy_reactive::classify_reactive_statement_body(
                &labeled.body,
                instance_source,
            );
            Ok(SupportedInstanceScriptItem::ReactiveStatement {
                body,
                deps: facts.deps,
                assignments: facts.assignments,
                span,
            })
        }
        // Every OTHER NON-variable top-level statement fails closed with its construct
        // label. The labels are precise so the completeness gate can pin each family.
        other => Err(UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
            construct: top_level_statement_label(other),
            span: stmt_span(other),
        }),
    }
}

/// Classify a top-level `export …` NAMED-export statement — the export-family
/// split:
///
/// - `export let <ident>[= default][, …]` under LEGACY mode → the
///   [`SupportedInstanceScriptItem::ExportLetProps`] prop item (one emitted
///   `$.prop` declaration per declarator). Under RUNES mode the SAME statement
///   is the official `legacy_export_invalid` compile error — returned through
///   the [`UnsupportedSvelteRuntimeSurface::OfficialReject`] carrier so the
///   accept site itself is mode-gated (a runes-mode `export let` can never take
///   legacy lowering).
/// - `export let` with a DESTRUCTURED / TS-annotated declarator → the precise
///   fail-closed sibling refusal (official accepts the destructured form as a
///   lazy-default prop surface — a deferral, not a reject).
/// - `export const` / `export function` / `export class` (ANY mode) → the
///   own-identity [`UnsupportedSvelteRuntimeSurface::ComponentExportBinding`]
///   refusal (the official `$$exports` accessor mechanism, not yet emitted).
/// - `export var` → its own precise fail-closed label (official lowers it as a
///   prop with the `var` keyword — a distinct deferred surface).
/// - A specifier list (`export { a }`) / any other named-export form → the
///   generic export residual.
fn classify_export_statement(
    export: &oxc_ast::ast::ExportNamedDeclaration<'_>,
    legacy: &LegacyScriptFacts,
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    use oxc_ast::ast::Declaration;
    let span = Span::new(export.span.start, export.span.end);
    let refuse = |construct: &'static str| {
        Err(UnsupportedSvelteRuntimeSurface::InstanceScriptItem { construct, span })
    };
    match &export.declaration {
        Some(Declaration::VariableDeclaration(decl)) => match decl.kind {
            VariableDeclarationKind::Let => {
                if !legacy.legacy_mode {
                    // A RUNES-mode `export let` is the official compile error —
                    // the mode-gated accept site (the pre-lowering gate and the
                    // classifier's runes-side gate normally reject first; this
                    // arm keeps the accept itself airtight).
                    return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                        rejection: super::official_rule::OfficialRejection::of(
                            super::official_rule::CoreOfficialValidationRule::LegacyExportInvalid,
                        ),
                        span,
                    });
                }
                let mut locals = Vec::new();
                for d in &decl.declarations {
                    if d.type_annotation.is_some() || d.definite {
                        return refuse("ts-annotated export let");
                    }
                    match &d.id {
                        BindingPattern::BindingIdentifier(id) => {
                            locals.push(id.name.to_string());
                        }
                        // Official accepts a destructured `export let` as a
                        // lazy-default prop surface — OUT of the supported
                        // export-let shape; the precise sibling refusal.
                        BindingPattern::ObjectPattern(_)
                        | BindingPattern::ArrayPattern(_)
                        | BindingPattern::AssignmentPattern(_) => {
                            return refuse("destructured export let");
                        }
                    }
                }
                Ok(SupportedInstanceScriptItem::ExportLetProps { locals })
            }
            // `export const` — the readonly `$$exports` component-export surface.
            VariableDeclarationKind::Const => {
                Err(UnsupportedSvelteRuntimeSurface::ComponentExportBinding {
                    construct: "const",
                    span,
                })
            }
            // `export var` — official lowers it as a PROP with the `var` keyword
            // (a distinct deferred surface, not the `$$exports` mechanism).
            VariableDeclarationKind::Var => refuse("export var declaration"),
            _ => refuse("export"),
        },
        Some(Declaration::FunctionDeclaration(_)) => {
            Err(UnsupportedSvelteRuntimeSurface::ComponentExportBinding {
                construct: "function",
                span,
            })
        }
        Some(Declaration::ClassDeclaration(_)) => {
            Err(UnsupportedSvelteRuntimeSurface::ComponentExportBinding {
                construct: "class",
                span,
            })
        }
        // A specifier list (`export { a }` / `export { a as b }` / a re-export)
        // and every other named-export form — the generic export residual.
        _ => refuse("export"),
    }
}

/// Whether a top-level expression is the production-ELIDED `$inspect` statement
/// shape: a `$inspect(...)` CallExpression (callee the bare identifier
/// `$inspect`), or a `$inspect(...).with(...)` chain (a CallExpression whose
/// callee is a static member `.with` whose object is itself a `$inspect(...)`
/// call). Driven from the typed OXC AST only. Any other shape — a longer chain
/// (`$inspect(x).with(f).g()`), a different member, a non-call — is NOT the
/// elision shape (the caller falls through to the fail-closed refusal).
fn is_inspect_elision_expression(expr: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = expr else {
        return false;
    };
    if is_inspect_callee(&call.callee) {
        return true;
    }
    // `$inspect(...).with(...)`: the callee is a static `.with` member on a
    // `$inspect(...)` call.
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if member.property.name.as_str() == "with" {
            if let Expression::CallExpression(inner) = &member.object {
                return is_inspect_callee(&inner.callee);
            }
        }
    }
    false
}

/// Whether a callee is the bare `$inspect` identifier.
fn is_inspect_callee(callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::Identifier(id) if id.name.as_str() == "$inspect")
}

/// Classify a top-level WELL-FORMED effect-family STATEMENT — `$effect(fn);` /
/// `$effect.pre(fn);` (the user-effect members' ONLY official-legal position)
/// or an UNASSIGNED bare `$effect.root(fn);` / `$effect.tracking();` (official
/// accepts the unassigned side-effect forms exactly like the assigned
/// declarator inits — oracle-verified, no frame) — into the
/// [`SupportedInstanceScriptItem::EffectStatement`] carrier. `None` for every
/// other expression, including every malformed form. Driven from the typed OXC
/// AST via the SHARED family statement classifier — author parens around the
/// call are transparent, and the slice is the CALL span, so the parens never
/// enter the carrier (the emission stays normalized); comment trivia inside
/// the peeled wrapper HEAD (`(/*#__PURE__*/ $effect(fn));` — between the
/// statement-expression start and the call start) pre-renders into the
/// carrier's `head_trivia`, and the carrier TAIL — the LEXICAL trailing
/// region collected by [`carrier_tail_comment_trivia`]: the statement span's
/// interior after the call end (a normalized wrapper's interior,
/// `($effect.root(fn) /*!license*/);`, and an unwrapped call's pre-`;`
/// trailing comments) PLUS the same-line ASI extension of a semicolon-less
/// statement (`$effect.root(fn) /*!license*/` — the OXC statement span ends
/// AT the call end, so the trailing comment lies outside every AST span) —
/// into `tail_trivia`, so the carrier slice never silently drops either.
fn classify_effect_statement(
    expr: &Expression<'_>,
    stmt_end: u32,
    instance_source: &str,
    comments: &[Comment],
) -> Option<SupportedInstanceScriptItem> {
    let fact = super::expr::effect_family_statement_fact(expr)?;
    let source = instance_source
        .get(fact.call_span.start as usize..fact.call_span.end as usize)?
        .to_string();
    let head_trivia = call_internal_comment_trivia(
        instance_source,
        comments,
        expr.span().start,
        fact.call_span.start,
    );
    let tail_trivia =
        carrier_tail_comment_trivia(instance_source, comments, fact.call_span.end, stmt_end);
    Some(SupportedInstanceScriptItem::EffectStatement {
        source,
        head_trivia,
        tail_trivia,
    })
}

/// Classify a top-level `function name(...) {}` declaration into the
/// [`SupportedInstanceScriptItem::FunctionDecl`] item, or fail closed.
///
/// Admitted ONLY when the function has a name AND that name is EXACTLY in the
/// function-pair-referenced set (the bare-identifier names referenced by an accepted DOM
/// `bind:value={get, set}` pair). An anonymous function declaration (no name) or one
/// whose name is NOT a function-pair reference fails closed at the instance-script-item
/// gate (construct `function`) — this is the precise gate, NOT a wildcard function path.
fn classify_function_declaration(
    func: &oxc_ast::ast::Function<'_>,
    instance_source: &str,
    bind_function_pair_names: &[String],
    store_admissions: &StoreScriptAdmissions,
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = || UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct: "function",
        span: Span::new(func.span.start, func.span.end),
    };
    let Some(name) = func.id.as_ref().map(|id| id.name.as_str()) else {
        // An anonymous top-level function declaration has no name to bind a pair to.
        return Err(refuse());
    };
    if !bind_function_pair_names.iter().any(|n| n == name)
        && !store_admissions.function_names.contains(name)
    {
        return Err(refuse());
    }
    let source = instance_source
        .get(func.span.start as usize..func.span.end as usize)
        .unwrap_or_default()
        .to_string();
    Ok(SupportedInstanceScriptItem::FunctionDecl {
        name: name.to_string(),
        source,
    })
}

/// Classify a top-level `class NAME { … }` declaration into the
/// [`SupportedInstanceScriptItem::StoreClassDecl`] item, or fail closed.
///
/// Admitted ONLY when the class has a name AND that name is in the `$store`
/// dependency closure (a local store CLASS reached transitively from a
/// subscribed base — `const c = new S(); {$c}` admits `S`). Its body emits
/// VERBATIM (official frames the store via `new`, class body unchanged). An
/// anonymous class (no name to bind), or one whose name is NOT a store-closure
/// dependency, fails closed at the instance-script-item gate (construct
/// `class`) — this is the precise gate, NOT a wildcard "emit any class" path.
fn classify_class_declaration(
    class: &oxc_ast::ast::Class<'_>,
    instance_source: &str,
    store_admissions: &StoreScriptAdmissions,
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = || UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct: "class",
        span: Span::new(class.span.start, class.span.end),
    };
    let Some(name) = class.id.as_ref().map(|id| id.name.as_str()) else {
        // An anonymous top-level class declaration has no name to bind a
        // subscription dependency to.
        return Err(refuse());
    };
    if !store_admissions.class_names.contains(name) {
        return Err(refuse());
    }
    // A class body carrying an inner `$`-store/rune reactive reference cannot be
    // lowered VERBATIM: official `svelte@5.56.3` rewrites an inner `$a` read to
    // `$a()` and an inner `$a = v` write to `$.store_set(a, v)` inside class
    // method / getter/setter bodies, field initializers, and static blocks — a
    // rewrite the verbatim `StoreClassDecl` emit does not perform. Fail closed on
    // any such class (the SIMPLE `subscribe`-bearing store class with NO inner
    // reactive surface stays supported via verbatim lowering).
    if super::store_subscriptions::class_body_has_inner_reactive_reference(class) {
        return Err(UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
            construct: "class with an inner $-reactive reference",
            span: Span::new(class.span.start, class.span.end),
        });
    }
    let source = instance_source
        .get(class.span.start as usize..class.span.end as usize)
        .unwrap_or_default()
        .to_string();
    Ok(SupportedInstanceScriptItem::StoreClassDecl {
        name: name.to_string(),
        source,
    })
}

/// Classify a top-level `VariableDeclaration` into shape 1/2/3/4, or fail closed.
///
/// A `var` / `const` declaration, a multi-declarator declaration, or any declarator
/// that is not exactly one of the four supported shapes fails closed.
#[allow(clippy::too_many_arguments)]
fn classify_instance_variable_decl(
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    instance_source: &str,
    comments: &[Comment],
    bind_this_targets: &[String],
    bind_lvalue_roots: &[String],
    store_admissions: &StoreScriptAdmissions,
    legacy: &LegacyScriptFacts,
    dispatcher_locals: &rustc_hash::FxHashSet<String>,
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = |construct: &'static str| UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct,
        span: Span::new(decl.span.start, decl.span.end),
    };
    // (0) An assignable effect-family EXPRESSION-rune init (`let`/`const name =
    // $effect.root(fn)` / `= $effect.tracking()`) is its own rewriter-backed
    // carrier — classified BEFORE the `let`-only gate because official accepts
    // BOTH keywords for it (the emission preserves the keyword). A `var`
    // declarator or any other shape falls through to the existing gates.
    if let Some(item) = classify_effect_rune_init(decl, instance_source, comments) {
        return Ok(item);
    }
    // (0a) A `$store` SOURCE `const` (`const count = writable(0)` with a `$count`
    // subscription, or a store DEPENDENCY reachable from a subscribed base) —
    // classified BEFORE the `let`-only gate. STORE-BOUND-ONLY: the admission set
    // is seeded exclusively by classified `$name` subscriptions, so an arbitrary
    // `const x = make()` with no subscription falls through to the existing
    // fail-closed const gate. Single identifier declarator, initialized, no TS
    // annotation — anything else falls through.
    if decl.kind == VariableDeclarationKind::Const {
        if let [d] = decl.declarations.as_slice() {
            if let BindingPattern::BindingIdentifier(id) = &d.id {
                let name = id.name.as_str();
                if store_admissions.const_names.contains(name)
                    && d.type_annotation.is_none()
                    && !d.definite
                {
                    if let Some(init) = &d.init {
                        let span = init.span();
                        if let Some(src) =
                            instance_source.get(span.start as usize..span.end as usize)
                        {
                            return Ok(SupportedInstanceScriptItem::StoreSourceDecl {
                                name: name.to_string(),
                                init: src.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    // (0a2) A DISPATCHER declarator (`const`/`let NAME = createEventDispatcher();`
    // — a zero-argument call of an admitted `svelte` `createEventDispatcher`
    // import local, single identifier declarator, no TS annotation) — the
    // component-event dispatcher carrier, classified BEFORE the keyword gates
    // (official accepts both keywords; the emission preserves the keyword). Any
    // other shape (arguments, a TS type argument under `lang="ts"` — refused
    // upstream as TypeScript — a non-dispatcher callee) falls through to the
    // existing fail-closed gates.
    if matches!(
        decl.kind,
        VariableDeclarationKind::Const | VariableDeclarationKind::Let
    ) {
        if let [d] = decl.declarations.as_slice() {
            if let BindingPattern::BindingIdentifier(id) = &d.id {
                if d.type_annotation.is_none() && !d.definite {
                    if let Some(Expression::CallExpression(call)) = &d.init {
                        if call.arguments.is_empty()
                            && !call.optional
                            && call.type_arguments.is_none()
                        {
                            if let Expression::Identifier(callee) = &call.callee {
                                if dispatcher_locals.contains(callee.name.as_str()) {
                                    return Ok(SupportedInstanceScriptItem::DispatcherDecl {
                                        const_decl: decl.kind == VariableDeclarationKind::Const,
                                        name: id.name.to_string(),
                                        callee: callee.name.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // (0b) A `$props.id()` declarator (`let`/`const <name> = $props.id();`, plus
    // literal-only siblings) is the hoisted-id carrier — classified BEFORE the
    // `let`-only gate because official accepts both keywords (the hoisted decl is
    // always `const`). A `var` / TS-annotated / non-literal-sibling shape falls
    // through to the existing fail-closed gates.
    if let Some(item) = classify_props_id_decl(decl, instance_source) {
        return Ok(item);
    }
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
            classify_identifier_declarator(
                d,
                name,
                decl,
                instance_source,
                bind_this_targets,
                bind_lvalue_roots,
                legacy,
            )
        }
        BindingPattern::ObjectPattern(_) => {
            // The ONLY supported destructure is a `$props()` call destructure.
            // The detailed member shape — plain and `$bindable(...)` defaults
            // through the `$.prop` substrate, and a rest element
            // (`{ …, ...rest }`) through the `$.rest_props` capture; computed /
            // numeric-key / nested patterns refused — is enforced by
            // `props_shape` upstream; here the declarator must be a `$props()`
            // call destructure.
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
/// into shape 1 (`$state(<primitive>)`), shape 3 (bare `let el;` bind:this target), or
/// shape 4 (`let v = <literal init>;` DOM bind-target lvalue root), or fail closed.
fn classify_identifier_declarator(
    d: &oxc_ast::ast::VariableDeclarator<'_>,
    name: &str,
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    instance_source: &str,
    bind_this_targets: &[String],
    bind_lvalue_roots: &[String],
    legacy: &LegacyScriptFacts,
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = |construct: &'static str| UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct,
        span: Span::new(decl.span.start, decl.span.end),
    };
    // The LEGACY demand-driven promotion: a top-level `let` whose binding the
    // legacy finalizer flipped to `$.mutable_source` (it is WRITTEN — a script
    // or template reassign/member-mutation, or a two-way `bind:` target) is the
    // promoted signal item. Checked BEFORE the runes-shaped bind-local arms so a
    // legacy bind-target `let` takes the mutable-source lowering, never the
    // runes verbatim shape. The init (ANY expression) lowers through the shared
    // FALLIBLE rewriter at projection time; a rune-call init cannot reach here
    // (the legacy rune-reference gate refused it upstream). LEGACY-only by
    // construction: the promotion set is populated only under the final legacy
    // mode.
    if legacy.promoted_lets.contains(name) {
        use oxc_span::GetSpan;
        let init = d.init.as_ref().and_then(|init_expr| {
            let span = init_expr.span();
            instance_source
                .get(span.start as usize..span.end as usize)
                .map(str::to_string)
        });
        return Ok(SupportedInstanceScriptItem::MutableSourceLet {
            name: name.to_string(),
            init,
        });
    }
    match &d.init {
        // A bare `let name;` (no init): admitted as shape 3 (a `bind:this` clone-root
        // local) OR as the no-init plain-local DOM bind-target root (`let v; <input
        // bind:value={v}>` — official keeps the bare local verbatim and binds it with the
        // plain `() => v` / `($$value) => v = $$value` closures). An unused / plain bare
        // local that is NEITHER a `bind:this` target NOR a DOM bind-lvalue root fails
        // closed.
        None => {
            if bind_this_targets.iter().any(|t| t == name) {
                Ok(SupportedInstanceScriptItem::BindThisLocal {
                    name: name.to_string(),
                })
            } else if bind_lvalue_roots.iter().any(|t| t == name) {
                Ok(SupportedInstanceScriptItem::BindLocalLet {
                    name: name.to_string(),
                    init: None,
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
            // A `$derived` identifier declarator init is a deferral. A `$props()`
            // whole-object identifier binding (`let all = $props()`) is the
            // prefix-only `$.rest_props` capture — it lowers through the SAME
            // destructure path as an object pattern (the `props_shape` gate already
            // accepted it as a basic shape).
            if is_derived_callee(&call.callee) {
                return Err(refuse("$derived declarator"));
            }
            if is_props_callee(&call.callee) {
                return Ok(SupportedInstanceScriptItem::PropsDestructure);
            }
            // A plain non-rune call init (`let x = makeIt()`) is not core. This is
            // also the CARRIER a `let s = $state.snapshot(c)` instance-script
            // initializer rides — it fails closed here as the plain-call-init carrier,
            // NOT a rune refusal (the snapshot rune member is rewritten in every
            // supported position; only its plain-call-INIT carrier is deferred).
            // TODO(follow-up): support instance-script CALL initializers (`let s =
            // foo()` / `let s = $state.snapshot(c)` → `let s = $.snapshot($.get(c))`)
            // — owned by the instance-script-call-init carrier surface (a separate
            // future block); closes when that carrier lands.
            Err(refuse("plain let with call init"))
        }
        // Shape 4: a plain non-rune `let v = <literal-only init>` used SOLELY as a DOM
        // bind-target lvalue ROOT (a `bind:value={v}` ident, or the root of a
        // `bind:value={v.x}` member). Official keeps the plain local verbatim, so it is
        // admitted ONLY when (a) its name is a recorded bind-lvalue root AND (b) the
        // init is a LITERAL-ONLY value (so the verbatim emit is correct without an init
        // rewrite). A plain local NOT used as a bind target, or one with a
        // signal-bearing / identifier-bearing init (which official would `$.get`-
        // rewrite — a distinct surface), fails closed.
        Some(init_expr) => {
            if !bind_lvalue_roots.iter().any(|t| t == name) {
                // A plain local that is not a DOM bind-target root is not core — a
                // template read is only a reactive `$state` signal or a no-default prop.
                return Err(refuse("plain let"));
            }
            if !init_is_literal_only(init_expr) {
                // A bind-target plain local whose init is NOT literal-only (it
                // references an identifier / member / call — which official rewrites)
                // is a distinct surface; fail closed rather than emit it verbatim wrong.
                return Err(refuse("plain let with non-literal init"));
            }
            use oxc_span::GetSpan;
            let span = init_expr.span();
            let init = instance_source
                .get(span.start as usize..span.end as usize)
                .unwrap_or_default()
                .to_string();
            Ok(SupportedInstanceScriptItem::BindLocalLet {
                name: name.to_string(),
                init: Some(init),
            })
        }
    }
}

/// Whether an init expression is a LITERAL-ONLY value — a string / number / boolean /
/// null / bigint / regexp / template-with-no-substitution literal, a unary `+`/`-`/`~`
/// over a literal, OR an object / array literal whose every element/property VALUE is
/// recursively literal-only. A literal-only init carries NO identifier / member / call
/// reference, so it has no signal read official would `$.get`-rewrite — it is safe to
/// emit VERBATIM as a plain-local declaration.
///
/// An identifier / member / call / arrow / `this` / etc. init is NOT literal-only (it
/// could read a reactive binding), so a plain-local bind target with such an init
/// fails closed (a distinct surface), never a verbatim mis-emit.
pub(super) fn init_is_literal_only(expr: &Expression<'_>) -> bool {
    use oxc_ast::ast::{Expression as E, PropertyKey};
    match expr {
        E::StringLiteral(_)
        | E::NumericLiteral(_)
        | E::BooleanLiteral(_)
        | E::NullLiteral(_)
        | E::BigIntLiteral(_)
        | E::RegExpLiteral(_) => true,
        // A template literal is literal-only iff it has NO `${...}` substitution.
        E::TemplateLiteral(t) => t.expressions.is_empty(),
        // A unary `-1` / `+1` / `~0` / `!true` over a literal-only argument.
        E::UnaryExpression(u) => init_is_literal_only(&u.argument),
        // A parenthesized literal-only value.
        E::ParenthesizedExpression(p) => init_is_literal_only(&p.expression),
        // An array literal: every (present, non-spread) element must be literal-only.
        E::ArrayExpression(arr) => arr.elements.iter().all(|el| match el {
            oxc_ast::ast::ArrayExpressionElement::SpreadElement(_) => false,
            oxc_ast::ast::ArrayExpressionElement::Elision(_) => true,
            other => other.as_expression().is_some_and(init_is_literal_only),
        }),
        // An object literal: every property must be a non-computed plain key with a
        // literal-only value (no shorthand identifier, no spread, no computed key).
        E::ObjectExpression(obj) => obj.properties.iter().all(|prop| match prop {
            oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) => {
                !p.computed
                    && !p.shorthand
                    && matches!(
                        &p.key,
                        PropertyKey::StaticIdentifier(_)
                            | PropertyKey::StringLiteral(_)
                            | PropertyKey::NumericLiteral(_)
                    )
                    && init_is_literal_only(&p.value)
            }
            // A spread property (`{ ...x }`) reads `x` — not literal-only.
            oxc_ast::ast::ObjectPropertyKind::SpreadProperty(_) => false,
        }),
        _ => false,
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
