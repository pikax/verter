//! The Svelte runtime SEMANTIC IR taxonomy.
//!
//! This is a SEMANTIC pre-lowering surface — it carries template structure,
//! template scopes, expression / pattern identities, binding classifications,
//! event policy, static/dynamic attribute facts, and block/snippet boundaries.
//! It does NOT carry any backend artifact: no client DOM variable names, no
//! `$.`-call strings, no SSR `$$renderer.push` text, no import text, no
//! sourcemaps — those belong to the client / server backends that consume this
//! IR.
//!
//! The IR is arena-backed (Vec arenas indexed by `u32` newtype ids), mirroring
//! the Vapor codegen's index-based pattern. Nodes, runtime ops, expressions,
//! patterns, scopes, and template scopes each live in their own arena on
//! [`SvelteRuntimeIr`] / [`RuntimeAnalysis`]; a structural node references its
//! children / scopes by id, never by owning pointer.

use verter_span::Span;

use super::expr::{BindingTable, ExprArena, ScopeGraph, ScopeId, ScriptAnalysis};

/// A node in the template-node arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// A runtime op in the op arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OpId(pub u32);

/// A template-expression in the expression arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

/// A binding pattern (each/await/snippet/declaration-tag) in the pattern arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PatternId(pub u32);

/// A template scope in the template-scope arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TemplateScopeId(pub u32);

/// The reactivity mode a component compiles under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteMode {
    /// Svelte 5 runes mode (`$state`/`$derived`/`$props`/…).
    Runes,
    /// Legacy non-runes mode (`export let`/`$:`/store auto-subscriptions).
    Legacy,
}

/// How an interpolation's text is escaped on insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeMode {
    /// HTML-escaped text content (the default `{expr}` interpolation).
    Escaped,
    /// Raw markup (`{@html expr}`) — inserted without escaping.
    Raw,
}

/// The whole component's identity + mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentIr {
    /// The component-function name — derived from the filename stem (JS-identifier
    /// sanitized) or an explicit name override.
    pub name: String,
    /// The compile FILENAME (the `filename` compile option, verbatim), used as the
    /// input to the `$.head('<hash>', …)` scope hash a `<svelte:head>` emits (the
    /// official `hash(filename)`). `None` when the host supplied no filename.
    pub filename: Option<String>,
    /// The reactivity mode.
    pub mode: SvelteMode,
}

/// The component-wide analysis facts: the two scripts, the expression arena, the
/// lexical scope graph, the binding table, and the binding-pattern arena.
///
/// The reactivity mode is owned solely by [`ComponentIr::mode`] — it is NOT
/// duplicated here or on [`ScriptAnalysis`].
#[derive(Debug)]
pub struct RuntimeAnalysis<'a> {
    /// The instance + module script analysis.
    pub scripts: ScriptAnalysis<'a>,
    /// The reparsed, scope-annotated template-expression arena.
    pub expressions: ExprArena<'a>,
    /// The lexical scope graph (script + expression-local + template scopes).
    pub scopes: ScopeGraph,
    /// The binding table classifying every relevant binding.
    pub bindings: BindingTable,
    /// The binding-pattern arena, indexed by [`PatternId`]: each entry is the
    /// ordered list of declared binding ids a pattern introduces. Retained on the
    /// analysis (NOT dropped with the lowering context) so a backend can resolve a
    /// each / await / snippet / declaration-tag [`PatternId`] after lowering
    /// returns.
    pub patterns: Vec<PatternBindings>,
}

/// The declared binding ids of one binding pattern, in source order.
///
/// A plain-identifier pattern declares one id; a destructuring pattern
/// (`{a, b}` / `[x, y]`) declares one id PER declared name, so `{@const {a, b} =
/// obj}` resolves to two distinct bindings rather than collapsing onto one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatternBindings {
    /// The declared binding ids in source order.
    pub bindings: Vec<BindingId>,
}

/// One template scope: a lexical region of the template (the root, an `{#each}`
/// body, an `{#await}` branch, a `{#snippet}` body, …) owning its scope id, its
/// root nodes, and the runtime ops local to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateScope {
    /// The lexical scope this template region binds in.
    pub scope: ScopeId,
    /// The template region's root nodes in source order.
    pub roots: Vec<NodeId>,
    /// The runtime ops local to this scope.
    pub local_ops: Vec<OpId>,
}

/// A template node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrNode {
    /// A literal text run.
    Text {
        /// The source span of the text.
        span: Span,
        /// The text content.
        text: String,
    },
    /// An HTML comment.
    Comment {
        /// The source span (inner content, excluding the delimiters).
        span: Span,
        /// The comment text.
        text: String,
    },
    /// A `{expr}` interpolation.
    Interpolation {
        /// The source span of the inner expression.
        span: Span,
        /// The interpolated expression.
        expr: ExprId,
        /// How the inserted text is escaped.
        escape: EscapeMode,
    },
    /// A regular intrinsic element.
    Element(ElementIr),
    /// A component reference (`<Foo>` / `<Foo.Bar>`).
    Component(ComponentIrNode),
    /// A `<svelte:*>` special element.
    Special(SpecialElementIr),
    /// A block construct (`{#if}` / `{#each}` / `{#await}` / `{#key}` /
    /// `{#snippet}`).
    Block(BlockIr),
    /// A standalone tag (`{@render}` / `{@html}` / `{@const}` / declaration tags
    /// / `{@debug}` / `{@attach}`).
    Tag(TagIr),
}

/// An intrinsic element node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementIr {
    /// The tag name (`div`, `button`, …).
    pub tag: String,
    /// The full open-tag source span.
    pub span: Span,
    /// The element's attributes / directives.
    pub attrs: Vec<AttrIr>,
    /// The element's children in source order.
    pub children: Vec<NodeId>,
    /// The lexical scope the element's children bind in.
    pub scope: ScopeId,
}

/// A component-reference node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentIrNode {
    /// The component reference name (`Foo`, `Foo.Bar`).
    pub name: String,
    /// The full open-tag source span.
    pub span: Span,
    /// The component's attributes (props / events / spreads / binds / `let:`).
    pub attrs: Vec<AttrIr>,
    /// The component's children (ALL of them) in source order — the structural mirror
    /// plus the source for the slot partition in [`ComponentSlots`]. The slot-content
    /// children ALSO appear in their slot region's roots (a default / named-slot
    /// region), and the `{#snippet}` child defs in [`ComponentSlots::snippet_defs`].
    pub children: Vec<NodeId>,
    /// The lexical scope the component's children bind in.
    pub scope: ScopeId,
    /// The component's slot decomposition — the default-slot region, named-slot
    /// regions, and `{#snippet}`-child definitions (built at lowering).
    pub slots: ComponentSlots,
}

/// The slot decomposition of a component / component-family special — the official
/// `Component.js` child grouping (`{#snippet}` defs hoisted to local consts + props,
/// `slot=`-bearing children into named slots, the rest into the default slot).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentSlots {
    /// The default-slot content region (everything not a `{#snippet}` def or a
    /// `slot=`-bearing child), or `None` when the default slot is empty.
    pub default: Option<TemplateScopeId>,
    /// The `let:` slot-prop directives on the component itself — applied to the DEFAULT
    /// slot (each lowers to `const <name> = $.derived(() => $$slotProps.<key>)` prepended
    /// to the default-slot callback). Empty for a component with no `let:`.
    pub default_lets: Vec<LetBinding>,
    /// The named slots (a `<svelte:fragment slot="x">` or a `slot="x"`-bearing child),
    /// in source order — each its own content region.
    pub named: Vec<NamedSlot>,
    /// The `{#snippet}` definitions declared DIRECTLY as component children — hoisted to
    /// local consts and passed as shorthand props (`header` + `$$slots.header: true`).
    pub snippet_defs: Vec<NodeId>,
    /// Whether any `let:` slot-prop directive on the component OR a named-slot child used an
    /// UNSUPPORTED form — a destructuring / non-identifier alias (`let:item={{a, b}}`) or a
    /// non-expression value. The shorthand `let:item` and the simple-identifier alias
    /// `let:item={alias}` decompose into [`default_lets`](Self::default_lets) /
    /// [`NamedSlot::lets`]; any other form sets this flag so the (fallible) component
    /// projection fails CLOSED, never silently dropping the binding — the let decomposition
    /// itself is infallible at lowering, so it carries this fact to the projection gate.
    pub has_unsupported_let: bool,
}

/// One `let:` slot-prop directive on a component — the local binding name and the
/// `$$slotProps` key it derives from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetBinding {
    /// The introduced local binding name — `item` for the shorthand `let:item`, or the
    /// alias `value` for `let:item={value}`.
    pub name: String,
    /// The `$$slotProps` key the local derives from — `item` for BOTH `let:item` (where
    /// `key == name`) and the aliased `let:item={value}` (where `key` is `item` and `name`
    /// is the alias `value`). Each lowers to `const <name> = $.derived(() =>
    /// $$slotProps.<key>)`.
    pub key: String,
}

/// A named slot on a component — the slot name and its content region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSlot {
    /// The slot name (`header` in `slot="header"`).
    pub name: String,
    /// The slot-content region.
    pub region: TemplateScopeId,
    /// The named slot's OWN `let:` slot-prop bindings (shorthand `let:item` or aliased
    /// `let:item={alias}`), carried at PLAN time so the slot-callback emitter consumes the
    /// planned fact directly and never rescans the IR / binding table for them.
    pub lets: Vec<LetBinding>,
}

/// A `<svelte:*>` special-element node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecialElementIr {
    /// The special-element kind.
    pub kind: SpecialKind,
    /// The full open-tag source span.
    pub span: Span,
    /// The special element's attributes / directives.
    pub attrs: Vec<AttrIr>,
    /// The DYNAMIC-TAG expression of a `<svelte:element this={…}>` (the runtime tag
    /// name) or a `<svelte:component this={C}>` (the runtime component). This is the
    /// element/component SELECTOR — NOT a DOM attribute — so it is carried as a
    /// distinct fact here and is EXCLUDED from [`SpecialElementIr::attrs`] (official
    /// `SvelteElement` reads `node.tag` / `SvelteComponent` reads `node.expression`,
    /// never an attribute named `this`). `None` for every other special kind (and for
    /// a `<svelte:element>` with no `this`, which is a parse error upstream).
    pub this_expr: Option<ExprId>,
    /// The STATIC `this="div"` tag literal of a `<svelte:element this="div">` (the decoded
    /// literal, NOT JS-quoted) — the runtime tag a `() => 'div'` get-tag thunk emits. `None`
    /// for a DYNAMIC `this={…}` (carried by [`Self::this_expr`]) and every non-element
    /// special. Mutually exclusive with `this_expr`.
    pub static_tag: Option<String>,
    /// The special element's children in source order.
    pub children: Vec<NodeId>,
    /// The lexical scope the special element's children bind in.
    pub scope: ScopeId,
    /// The slot decomposition — used by the component-family specials
    /// (`<svelte:component>` / `<svelte:self>` / `<svelte:fragment>`); empty (the
    /// `Default`) for the host / renderable specials.
    pub slots: ComponentSlots,
    /// The RENDERABLE special's CHILD-CONTENT region — its own template scope (the children
    /// render INSIDE the special's callback, NOT in the enclosing region). `Some` for the
    /// renderable specials whose body is a callback region (`<svelte:element>` /
    /// `<svelte:boundary>` / `<svelte:head>`); `None` for the host / component-family
    /// specials (whose children, if any, are not a callback region).
    pub body_region: Option<TemplateScopeId>,
    /// The `<svelte:head>`'s `<title>` child, decomposed into its template chunks (the
    /// official `TitleElement` reads `node.fragment.nodes`). `Some` for a `<svelte:head>`
    /// that carries a `<title>`; `None` otherwise. The title is NOT a body-region DOM node
    /// (it renders no element — it drives `$.document.title`); its non-title siblings
    /// (`<meta>` / `<link>` / …) are the `body_region`.
    pub head_title: Option<HeadTitleIr>,
}

/// A `<svelte:head>`'s `<title>` element decomposed into its template chunks — the input
/// the projector folds into the `$.document.title = <rhs>` write (the official
/// `TitleElement` + `build_template_chunk`). Carries NO DOM node: the title renders no
/// element, so it is not a body-region root and produces no reactive-text op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadTitleIr {
    /// The title fragment's chunks in source order (literal text runs + interpolation
    /// expressions), mirroring `node.fragment.nodes`.
    pub chunks: Vec<TitleChunkIr>,
    /// The span of the FIRST parsed title child that is NEITHER text NOR an interpolation
    /// (a nested element, comment, or block) — the official `title_invalid_content` error
    /// (`<title>` can only contain text and `{tags}`). `None` for a title whose children are
    /// exclusively text + interpolations; a `Some` fails the surface closed at classification.
    pub invalid_content: Option<Span>,
}

/// One chunk of a `<title>`'s content — a literal text run or an interpolation expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleChunkIr {
    /// A literal (entity-decoded) text run.
    Text(String),
    /// An `{expr}` interpolation (an id into the expression arena).
    Expr(ExprId),
}

/// The closed family of `<svelte:*>` special-element kinds the runtime IR
/// distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKind {
    /// `<svelte:head>`.
    Head,
    /// `<svelte:window>`.
    Window,
    /// `<svelte:document>`.
    Document,
    /// `<svelte:body>`.
    Body,
    /// `<svelte:element this={…}>`.
    Element,
    /// `<svelte:boundary>`.
    Boundary,
    /// `<svelte:options>`.
    Options,
    /// `<svelte:component this={C}>`.
    Component,
    /// `<svelte:self>`.
    SelfRef,
    /// `<svelte:fragment>`.
    Fragment,
}

/// A static (compile-time-constant) attribute value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticAttrValue {
    /// The literal attribute value.
    pub value: String,
}

/// Which authored syntax an [`AttrIr::Event`] came from. The MODERN Svelte-5 `on*`
/// attribute (`onclick={…}`) and the LEGACY `on:` directive (`on:click={…}`) collapse to
/// the SAME `AttrIr::Event` once normalized, so the distinct origin is recorded at lowering
/// (where the syntax is still separate) for the few consumers whose validity differs by form
/// — e.g. `<svelte:boundary>`, which accepts only the modern `onerror` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventOrigin {
    /// The MODERN Svelte-5 `on*` attribute form (`onclick={…}` / `onerror={…}`).
    ModernAttribute,
    /// The LEGACY `on:` directive form (`on:click={…}` / `on:error|preventDefault={…}`).
    LegacyDirective,
}

/// An attribute or directive on an element / component / special element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrIr {
    /// A static attribute with a literal value (`class="x"`) — folded into the
    /// static template HTML.
    Static {
        /// The attribute name.
        name: String,
        /// The literal value (`None` for a valueless boolean attribute).
        value: Option<StaticAttrValue>,
    },
    /// A dynamic attribute whose value is a reactive expression
    /// (`id={expr}` / shorthand `{value}`).
    Dynamic {
        /// The attribute name.
        name: String,
        /// The value expression.
        expr: ExprId,
    },
    /// A mixed / concatenated attribute value (`class="a {b}"`) — an ordered run
    /// of literal text + reactive expression parts the backend concatenates into
    /// the final value, instead of one un-parseable expression.
    Mixed {
        /// The attribute name.
        name: String,
        /// The ordered literal / expression parts.
        parts: Vec<MixedAttrPart>,
    },
    /// A spread attribute (`{...rest}`).
    Spread {
        /// The spread expression.
        expr: ExprId,
    },
    /// A `class:name={cond}` directive.
    Class {
        /// The class name (the part after `class:`).
        name: String,
        /// The condition expression, or `None` for the shorthand `class:name`
        /// (the condition is the same-named binding).
        condition: Option<ExprId>,
    },
    /// A `style:prop={value}` directive.
    Style {
        /// The style property name.
        property: String,
        /// The value: an EXPRESSION (`style:color={x}` / shorthand `style:color`) or a
        /// STATIC-TEXT literal (`style:color="red"` — the SOLE directive family that
        /// accepts a text value; it folds as a quoted string).
        value: StyleDirectiveValue,
        /// Whether the `|important` modifier was present.
        important: bool,
    },
    /// A `bind:` two-way binding directive.
    Bind {
        /// The bind target (the part after `bind:`).
        target: String,
        /// The bound expression, or `None` for the shorthand `bind:value`.
        expr: Option<ExprId>,
    },
    /// An event handler — Svelte-5 lowercase attribute form (`onclick={…}`) or
    /// legacy `on:` directive form. Carries the event-delegation policy.
    Event {
        /// The normalized event type (`click`, `input`, …) — the trailing
        /// `capture` suffix is stripped here (a capture handler carries the bare
        /// name plus `capture: true`).
        event_type: String,
        /// The handler expression.
        handler: ExprId,
        /// Whether the event type is delegated (vs a direct listener). A capture
        /// handler is NEVER delegated (the delegation decision keys on the RAW
        /// pre-normalization attribute name, which the trailing `capture` excludes
        /// from the delegated set).
        delegated: bool,
        /// Whether this is a CAPTURE-phase handler — set by the Svelte-5
        /// `*capture` attribute suffix (`onclickcapture`) or the legacy
        /// `on:click|capture` modifier. `gotpointercapture`/`lostpointercapture`
        /// are NOT capture events despite the suffix (the official
        /// `is_capture_event` exclusion).
        capture: bool,
        /// The legacy `on:` event modifiers (`|stopPropagation`, `|preventDefault`,
        /// …), empty for the Svelte-5 attribute form. (`|capture` is reflected in
        /// `capture`, but is also kept here for fidelity.)
        modifiers: Vec<String>,
        /// The resolved passive-listener option — the 5th positional `$.event` /
        /// `$.delegated` argument: `Some(true)` (passive), `Some(false)`
        /// (nonpassive), `None` (omitted). Computed at LOWERING (where the
        /// modern-vs-legacy form is known) so the emitter never re-infers it: the
        /// MODERN attribute form defaults to `is_passive_event(event_type)`
        /// (`touchstart` / `touchmove` ⇒ `Some(true)`), the LEGACY directive form
        /// derives it from the `|passive` / `|nonpassive` modifiers ONLY.
        passive: Option<bool>,
        /// Which authored syntax produced this event — the MODERN Svelte-5 `on*`
        /// attribute (`onclick={…}`) or the LEGACY `on:` directive (`on:click={…}`).
        /// Recorded at LOWERING because the two forms COLLAPSE to an identical
        /// `AttrIr::Event` once normalized (a bare `on:click` carries no modifiers,
        /// byte-identical to `onclick`), so this is the sole faithful discriminator
        /// downstream. Consumed where the two forms have DIFFERENT validity — a
        /// `<svelte:boundary>` accepts ONLY the modern `onerror` attribute and rejects
        /// every legacy `on:` directive (official `svelte_boundary_invalid_attribute`)
        /// — AND where they have different EMISSION semantics: on a regular element the
        /// LEGACY `on:` form never delegates, joins the element's post-walk DIRECTIVE
        /// BATCH (source-ordered with `transition:` / `animate:`, official
        /// `RegularElement.js` `other_directives` → `element_state.after_update`), and
        /// effect-wraps into the init domain on a `use:` action host; the MODERN
        /// attribute form emits BEFORE that batch and never effect-wraps (`delegated` /
        /// `modifiers` carry the remaining per-form differences).
        origin: EventOrigin,
    },
    /// A `use:action` directive (`use:fn` / `use:fn={arg}`).
    Use {
        /// The action-function expression (the synthesized `fn` reference).
        expr: ExprId,
        /// The action argument expression, if present (`use:fn={arg}`).
        arg: Option<ExprId>,
    },
    /// A `transition:` / `in:` / `out:` directive (`animate:` is its OWN
    /// [`Animate`](Self::Animate) family — a distinct runtime helper, `$.animation`).
    Transition {
        /// The transition kind.
        kind: TransitionKind,
        /// The transition name (the part after the prefix).
        name: String,
        /// The transition argument expression, if present.
        expr: Option<ExprId>,
        /// Whether the `|global` modifier is present — the official
        /// `TRANSITION_GLOBAL` (4) flag bit. `|local` (or no modifier) is the
        /// default and carries `false`.
        global: bool,
    },
    /// An `animate:` directive (keyed-each-only; the official `$.animation` helper —
    /// NEVER a transition).
    Animate {
        /// The animation function name (the part after `animate:`).
        name: String,
        /// The animation argument expression, if present.
        expr: Option<ExprId>,
    },
    /// An element-position `{@attach expr}` attachment (the official `AttachTag`
    /// in attribute position — `$.attach(el, () => expr)`). The CHILD-position form
    /// (`<div>{@attach expr}</div>`) is a parse-level official reject
    /// (`expected_tag`) and never lowers to this.
    Attach {
        /// The attachment expression.
        expr: ExprId,
    },
    /// A `let:` slot-prop binding directive.
    Let {
        /// The slot-prop name.
        name: String,
        /// The aliasing expression, or `None` for the shorthand `let:item`.
        expr: Option<ExprId>,
    },
}

/// The value of a `style:` directive — the SOLE directive family that accepts a
/// static-text value (every other directive REJECTS a text value as the official
/// `directive_invalid_value`).
///
/// - [`Expr`](Self::Expr) — `style:color={x}` (a bare expression), the quoted single-`{x}`
///   form (`style:color="{x}"`), OR the shorthand `style:color` (the implied `color`
///   reference). Folds as `{ color: <rewritten expr> }`.
/// - [`Text`](Self::Text) — `style:color="red"` (a static-text body). Carries the DECODED
///   text (NOT JS-quoted); the projector emits the single-quoted string literal
///   (`{ color: 'red' }`). A text-only style directive has NO `ExprId` — there is no
///   synthetic string-literal expression.
/// - [`Mixed`](Self::Mixed) — `style:color="a{x}b"` (a text + interpolation concatenation,
///   the SOLE directive family that accepts one). Carries the ordered literal / expression
///   parts; the projector folds the template-literal `` { color: `a${x ?? ''}b` } `` through
///   the SAME shared mixed-value + fold-text path a mixed plain attribute uses. A quoted
///   single-`{x}` value (`style:color="{x}"`) is NOT mixed — it is one `Expr` chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleDirectiveValue {
    /// An expression value (`style:color={x}` / shorthand `style:color`).
    Expr(ExprId),
    /// A static-text value (`style:color="red"`) — the DECODED text, NOT JS-quoted.
    Text(String),
    /// A MIXED text + interpolation value (`style:color="a{x}b"`) — the ordered
    /// literal / `{expr}` parts the projector concatenates into the template-literal
    /// `` `a${x ?? ''}b` ``.
    Mixed(Vec<MixedAttrPart>),
}

/// One part of a mixed / concatenated attribute value (`class="a {b}"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixedAttrPart {
    /// A literal text run (`a `).
    Literal(String),
    /// A reactive `{expr}` interpolation run.
    Expr(ExprId),
}

/// The closed family of transition-directive kinds (`animate:` is NOT a transition —
/// it is the distinct [`AttrIr::Animate`] family emitting `$.animation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    /// `transition:` (bidirectional).
    Transition,
    /// `in:` (one-way in).
    In,
    /// `out:` (one-way out).
    Out,
}

/// A reactive runtime op attached to a node or scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeOp {
    /// Reactive text content for a node.
    ReactiveText {
        /// The target node.
        target: NodeId,
        /// The text expression.
        expr: ExprId,
    },
    /// A reactive attribute write.
    ReactiveAttr {
        /// The target node.
        target: NodeId,
        /// The attribute op.
        attr: AttrOp,
    },
    /// A reactive spread-attributes write.
    SpreadAttrs {
        /// The target node.
        target: NodeId,
        /// The spread expressions in source order.
        spreads: Vec<ExprId>,
    },
    /// A `style:` directive TRIGGER for a value that is NOT a single reactive expression —
    /// a STATIC-TEXT body (`style:color="red"`) OR a MIXED text+interpolation body
    /// (`style:color="a{x}b"`). Neither carries a single `ExprId`, so the value cannot ride
    /// the expr-bearing [`RuntimeOp::ReactiveAttr`] path; the op is a pure marker that the
    /// element bears such a style directive, so the coalesced `$.set_style(node, …)`
    /// projection fires (even when every style directive is non-expression). The
    /// `style_done` dedup + `project_set_style_op` (which reads ALL the element's style
    /// attrs and re-derives reactivity, including the mixed template-literal) own the
    /// concrete call — this op never re-derives the value.
    StyleDirectiveTrigger {
        /// The target node.
        target: NodeId,
    },
    /// A two-way binding.
    Binding {
        /// The target node.
        target: NodeId,
        /// The binding op.
        bind: BindOp,
    },
    /// An event registration.
    Event {
        /// The event target.
        target: EventTarget,
        /// The event op.
        event: EventOp,
    },
    /// An `{@attach}` attachment.
    Attachment {
        /// The target node.
        target: NodeId,
        /// The attachment expression.
        expr: ExprId,
    },
    /// A `use:action` directive — an element action (`$.action`).
    Action {
        /// The target node.
        target: NodeId,
        /// The action expression (`use:fn` ⇒ `fn`; `use:fn={arg}` ⇒ the call
        /// surface the backend builds from the action + argument).
        action: ActionOp,
    },
    /// A `transition:` / `in:` / `out:` directive.
    Transition {
        /// The target node.
        target: NodeId,
        /// The transition op.
        transition: TransitionOp,
    },
    /// An `animate:` directive — a keyed-each item animation (`$.animation`).
    Animation {
        /// The target node.
        target: NodeId,
        /// The animation op.
        animation: AnimationOp,
    },
    /// A "cannot be set statically" attribute init (`autofocus` / `muted` /
    /// `defaultValue` / `defaultChecked`). These attributes are EXCLUDED from the
    /// static `from_html` skeleton and applied at runtime via a property write
    /// (`node.muted = true`) or the `$.autofocus(node, value)` helper — the official
    /// `cannot_be_set_statically` set (`src/utils.js`). The IR REPRESENTS the init;
    /// the concrete `$.autofocus` / property-write string is the emitting backend's.
    NonStaticProperty {
        /// The target node.
        target: NodeId,
        /// The init op (the property name, the autofocus-vs-property kind, and the
        /// init value).
        property: NonStaticPropertyOp,
    },
}

/// A "cannot be set statically" attribute init op (`autofocus` / `muted` /
/// `defaultValue` / `defaultChecked`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonStaticPropertyOp {
    /// The attribute / property name (`autofocus`, `muted`, `defaultValue`,
    /// `defaultChecked`).
    pub name: String,
    /// Whether this is the `$.autofocus(node, value)` helper init (`autofocus`) or a
    /// DOM property write (`node.<name> = value`, for `muted` / `defaultValue` /
    /// `defaultChecked`).
    pub kind: NonStaticPropertyKind,
    /// The init value.
    pub value: NonStaticPropertyValue,
}

/// Whether a [`NonStaticPropertyOp`] is applied via `$.autofocus` or a DOM property
/// write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonStaticPropertyKind {
    /// The `$.autofocus(node, value)` helper (`autofocus`).
    Autofocus,
    /// A DOM property write `node.<name> = value` (`muted` / `defaultValue` /
    /// `defaultChecked`).
    DomProperty,
}

/// The init value of a [`NonStaticPropertyOp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonStaticPropertyValue {
    /// A valueless boolean attribute (`<input autofocus>` / `<video muted>`) — the
    /// init value is the literal `true`.
    Boolean,
    /// A static literal value (`<input defaultValue="x">` → the string `x`).
    Literal(String),
    /// A dynamic expression value (`<input defaultValue={x}>`).
    Expr(ExprId),
    /// A MIXED / concatenated value (`<input defaultValue="a {x} b">`) — the FULL
    /// ordered run of literal-text + reactive-expression parts the backend
    /// concatenates into the assigned value (official `build_attribute_value` emits
    /// the template-literal `\`a ${x ?? ''} b\``). The literal chunks are RETAINED
    /// (not collapsed to a lone expression), so the property write preserves the
    /// official literal/expr alternation.
    Mixed(Vec<MixedAttrPart>),
}

/// A `use:action` op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOp {
    /// The action expression. For `use:fn` this is the synthesized `fn`
    /// reference; for `use:fn={arg}` it is the `fn` reference and the argument
    /// rides in `arg`.
    pub expr: ExprId,
    /// The action argument expression, if present (`use:fn={arg}`).
    pub arg: Option<ExprId>,
}

/// A transition op (`transition:` / `in:` / `out:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionOp {
    /// The transition kind.
    pub kind: TransitionKind,
    /// The transition function name.
    pub name: String,
    /// The transition argument expression, if present.
    pub expr: Option<ExprId>,
    /// Whether the `|global` modifier is present (the official `TRANSITION_GLOBAL`
    /// flag bit; `|local` / no modifier is the default `false`).
    pub global: bool,
}

/// An `animate:` op — the keyed-each item animation (`$.animation(el, () => fn,
/// PARAMS)`), a DISTINCT helper family from [`TransitionOp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationOp {
    /// The animation function name.
    pub name: String,
    /// The animation argument expression, if present.
    pub expr: Option<ExprId>,
}

/// A reactive attribute op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrOp {
    /// The attribute name.
    pub name: String,
    /// The value expression.
    pub expr: ExprId,
    /// The attribute reactive kind (plain attribute, class, style).
    pub kind: AttrOpKind,
}

/// The kind of reactive attribute op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOpKind {
    /// A plain dynamic attribute (`$.set_attribute`).
    Plain,
    /// A `class:` / `class={…}` op (`$.set_class`).
    Class,
    /// A `style:` / `style={…}` op (`$.set_style`).
    Style,
}

/// A two-way binding op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindOp {
    /// The bind target (`value`, `checked`, `this`, …).
    pub target: String,
    /// The bound expression.
    pub expr: ExprId,
}

/// An event-registration target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventTarget {
    /// A DOM node in the template.
    Node(NodeId),
    /// The `window` global (a `<svelte:window>` listener).
    Window,
    /// The `document` global (a `<svelte:document>` listener).
    Document,
    /// The `body` element (a `<svelte:body>` listener).
    Body,
}

/// An event-registration op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventOp {
    /// The normalized event type (`click`, `input`, …) — the trailing `capture`
    /// suffix is stripped (a capture handler carries the bare name plus
    /// `capture: true`).
    pub event_type: String,
    /// The handler expression.
    pub handler: ExprId,
    /// Whether the event is delegated. A capture handler is never delegated.
    pub delegated: bool,
    /// Whether this is a CAPTURE-phase handler (the `*capture` suffix or the
    /// legacy `|capture` modifier).
    pub capture: bool,
    /// The legacy `on:` modifiers (empty for the Svelte-5 attribute form).
    pub modifiers: Vec<String>,
    /// The resolved passive-listener option (the 5th positional `$.event` /
    /// `$.delegated` argument): `Some(true)` passive, `Some(false)` nonpassive,
    /// `None` omitted. Carried from lowering (the modern-vs-legacy passive rule).
    pub passive: Option<bool>,
    /// Which authored syntax produced this event (modern `on*` attribute vs legacy
    /// `on:` directive) — threaded from [`AttrIr::Event`]. The emission phase keys
    /// on it: a bare LEGACY event joins the post-walk directive batch and
    /// effect-wraps on a `use:` host; a MODERN event emits before that batch and
    /// never wraps.
    pub origin: EventOrigin,
}

/// A block construct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockIr {
    /// `{#if}` … `{:else if}` … `{:else}` … `{/if}`.
    If {
        /// The conditional branches in source order.
        branches: Vec<IfBranch>,
    },
    /// `{#each list as item, index (key)}` … `{:else}` … `{/each}`.
    Each {
        /// The list-source expression.
        source: ExprId,
        /// The item binding pattern, or `None` for the `{#each {length:n}}`
        /// no-item form.
        item: Option<PatternId>,
        /// The index binding pattern, if present.
        index: Option<PatternId>,
        /// The `(key)` expression, if present.
        key: Option<ExprId>,
        /// The block body template scope.
        body: TemplateScopeId,
        /// The `{:else}` template scope, if present.
        else_body: Option<TemplateScopeId>,
    },
    /// `{#await promise}` … `{:then v}` … `{:catch e}` … `{/await}`.
    Await {
        /// The awaited promise expression.
        promise: ExprId,
        /// The pending-branch template scope, if present.
        pending: Option<TemplateScopeId>,
        /// The `{:then <pattern>}` binding pattern, if present.
        then_binding: Option<PatternId>,
        /// The then-branch template scope, if present.
        then_body: Option<TemplateScopeId>,
        /// The `{:catch <pattern>}` binding pattern, if present.
        catch_binding: Option<PatternId>,
        /// The catch-branch template scope, if present.
        catch_body: Option<TemplateScopeId>,
    },
    /// `{#key expr}` … `{/key}`.
    Key {
        /// The key expression.
        expr: ExprId,
        /// The block body template scope.
        body: TemplateScopeId,
    },
    /// `{#snippet name(params)}` … `{/snippet}`.
    Snippet {
        /// The snippet's binding id.
        name: BindingId,
        /// The snippet's parameter patterns in source order.
        params: Vec<PatternId>,
        /// The snippet body template scope.
        body: TemplateScopeId,
    },
}

/// One branch of an `{#if}` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfBranch {
    /// The branch condition, or `None` for the trailing `{:else}` branch.
    pub condition: Option<ExprId>,
    /// The branch body template scope.
    pub body: TemplateScopeId,
}

/// A binding id into the [`BindingTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub u32);

/// The declaration-tag declaration kind (`{const …}` / `{let …}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    /// `{const …}` declaration tag.
    Const,
    /// `{let …}` declaration tag.
    Let,
}

/// One declarator of a `{const …}` / `{let …}` declaration tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDeclarator {
    /// The declarator's binding pattern.
    pub pattern: PatternId,
    /// The initializer expression, if present. For an INERT declarator this is the plain
    /// initializer; for a `$derived` rune declarator it is the `$derived` ARGUMENT (rewritten
    /// into the `$.derived(() => …)` body at projection); a `$state` rune declarator carries
    /// no init expr (its primitive text rides [`TemplateRune::State`]).
    pub init: Option<ExprId>,
    /// The declarator's rune classification, or `None` for an INERT declarator (a plain
    /// `{const}`/`{let}`). A rune declarator's binding is registered through the shared
    /// rune/state classification pipeline (so its template reads/writes route through the
    /// signal rewriter); a rune form the pipeline cannot lower stays inert and fails closed
    /// at the rewriter's advanced-rune gate.
    pub rune: Option<TemplateRune>,
}

/// The rune a `{let x = $state(…)}` / `{let x = $derived(…)}` declaration-tag declarator
/// carries — the typed classification that drives its emission. NEVER keyed by name: the
/// binding kind / lowering is resolved per declarator from its own binding id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateRune {
    /// `$state(<primitive>)` — the primitive init source text (inner `None` for the no-arg
    /// `$state()` ⇒ `void 0` form). The binding is reclassified through the write-gated
    /// `$state` pipeline (`$.state(<init>)` vs a never-reassigned plain `let`).
    State(Option<String>),
    /// `$derived(<arg>)` — a block-local derived memo. The argument expression rides
    /// [`TemplateDeclarator::init`]; the binding is a `Derived` signal (reads `$.get`).
    Derived,
}

/// The callee of a `{@render}` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderCallee {
    /// A static snippet name (`{@render row(n)}`) — a direct snippet call.
    Snippet(BindingId),
    /// A dynamic expression (`{@render expr?.()}`) — uses `$.snippet`.
    Dynamic(ExprId),
}

/// A standalone tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagIr {
    /// `{@render snippet(args)}`.
    Render {
        /// The render callee.
        callee: RenderCallee,
        /// The call arguments.
        args: Vec<ExprId>,
        /// `Some(span)` when the render call carries a SPREAD argument
        /// (`{@render row(...xs)}`) — official `svelte@5.56.3` HARD-ERRORS on it
        /// (`render_tag_invalid_spread_argument`), so the client-surface gate fails
        /// closed at this span rather than silently dropping the spread. `None` for a
        /// spread-free render call (the normal static/dynamic snippet-call surface).
        spread_arg_span: Option<Span>,
    },
    /// `{@html expr}`.
    Html {
        /// The raw-markup expression.
        expr: ExprId,
    },
    /// `{@const x = expr}` — the legacy-documented block-local derived memo. A
    /// destructuring `{@const {a, b} = obj}` introduces one binding per declared
    /// name (the pattern), NOT a single collapsed binding.
    LegacyConst {
        /// The introduced binding pattern (one binding row per declared name).
        pattern: PatternId,
        /// The initializer expression.
        init: ExprId,
    },
    /// `{const …}` / `{let …}` declaration tag — an inert block-local
    /// declaration (DISTINCT from `{@const}`).
    Declaration {
        /// The declaration kind.
        kind: DeclKind,
        /// The declarators in source order.
        declarators: Vec<TemplateDeclarator>,
    },
    /// `{@debug var1, var2}`.
    Debug {
        /// The debugged identifiers — each carries the PARSED identifier name (the
        /// emitted object key) plus its debugged expression (the snapshot argument), so
        /// the key is the typed identifier fact, never a re-sliced raw source span.
        args: Vec<DebugArg>,
    },
    /// `{@attach expr}`.
    Attach {
        /// The attachment expression.
        expr: ExprId,
    },
}

/// One `{@debug}` argument: the PARSED identifier name (the emitted `console.log({ … })`
/// object key) plus the debugged expression id (the `$.snapshot(<expr>)` argument). The
/// name is recovered from the OXC-parsed `IdentifierReference`, so a Unicode-escaped
/// identifier keys on its DECODED name rather than its raw source bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugArg {
    /// The decoded debug identifier name (the object key).
    pub name: String,
    /// The debugged expression (the snapshot argument).
    pub expr: ExprId,
}

/// The pre-lowering Svelte runtime IR.
///
/// The shared substrate the client + server backends build on: the component
/// identity + mode, the component-wide analysis, the root template scope, and
/// the four arenas (template scopes, nodes, ops, plus the expression / pattern /
/// scope arenas owned by [`RuntimeAnalysis`]).
#[derive(Debug)]
pub struct SvelteRuntimeIr<'a> {
    /// The component identity + mode.
    pub component: ComponentIr,
    /// The component-wide analysis.
    pub analysis: RuntimeAnalysis<'a>,
    /// The root template scope.
    pub root: TemplateScopeId,
    /// The template-scope arena, indexed by [`TemplateScopeId`].
    pub template_scopes: Vec<TemplateScope>,
    /// The template-node arena, indexed by [`NodeId`].
    pub nodes: Vec<IrNode>,
    /// The runtime-op arena, indexed by [`OpId`].
    pub ops: Vec<RuntimeOp>,
}

impl<'a> SvelteRuntimeIr<'a> {
    /// Look up a node by id.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &IrNode {
        &self.nodes[id.0 as usize]
    }

    /// The declared binding ids of a pattern.
    #[must_use]
    pub fn pattern_bindings(&self, id: PatternId) -> &[BindingId] {
        &self.analysis.patterns[id.0 as usize].bindings
    }

    /// Look up a template scope by id.
    #[must_use]
    pub fn template_scope(&self, id: TemplateScopeId) -> &TemplateScope {
        &self.template_scopes[id.0 as usize]
    }

    /// Look up a runtime op by id.
    #[must_use]
    pub fn op(&self, id: OpId) -> &RuntimeOp {
        &self.ops[id.0 as usize]
    }

    /// The root template scope.
    #[must_use]
    pub fn root_scope(&self) -> &TemplateScope {
        self.template_scope(self.root)
    }
}
