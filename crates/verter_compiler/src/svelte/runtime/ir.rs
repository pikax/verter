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
    /// The component's attributes (props / events / spreads / binds).
    pub attrs: Vec<AttrIr>,
    /// The component's children (default-slot content) in source order.
    pub children: Vec<NodeId>,
    /// The lexical scope the component's children bind in.
    pub scope: ScopeId,
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
    /// The special element's children in source order.
    pub children: Vec<NodeId>,
    /// The lexical scope the special element's children bind in.
    pub scope: ScopeId,
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
        /// The value expression, or `None` for the shorthand `style:prop`.
        value: Option<ExprId>,
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
        /// The legacy `on:` event modifiers (`|stop`, `|prevent`, …), empty for
        /// the Svelte-5 attribute form. (`|capture` is reflected in `capture`,
        /// but is also kept here for fidelity.)
        modifiers: Vec<String>,
    },
    /// A `use:action` directive (`use:fn` / `use:fn={arg}`).
    Use {
        /// The action-function expression (the synthesized `fn` reference).
        expr: ExprId,
        /// The action argument expression, if present (`use:fn={arg}`).
        arg: Option<ExprId>,
    },
    /// A `transition:` / `in:` / `out:` / `animate:` directive.
    Transition {
        /// The transition kind.
        kind: TransitionKind,
        /// The transition name (the part after the prefix).
        name: String,
        /// The transition argument expression, if present.
        expr: Option<ExprId>,
    },
    /// A `let:` slot-prop binding directive.
    Let {
        /// The slot-prop name.
        name: String,
        /// The aliasing expression, or `None` for the shorthand `let:item`.
        expr: Option<ExprId>,
    },
}

/// One part of a mixed / concatenated attribute value (`class="a {b}"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixedAttrPart {
    /// A literal text run (`a `).
    Literal(String),
    /// A reactive `{expr}` interpolation run.
    Expr(ExprId),
}

/// The closed family of transition-directive kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    /// `transition:` (bidirectional).
    Transition,
    /// `in:` (one-way in).
    In,
    /// `out:` (one-way out).
    Out,
    /// `animate:`.
    Animate,
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
    /// A `transition:` / `in:` / `out:` / `animate:` directive.
    Transition {
        /// The target node.
        target: NodeId,
        /// The transition op.
        transition: TransitionOp,
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

/// A transition / animation op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionOp {
    /// The transition kind.
    pub kind: TransitionKind,
    /// The transition/animation function name.
    pub name: String,
    /// The transition argument expression, if present.
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
    /// The initializer expression, if present.
    pub init: Option<ExprId>,
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
        /// The debugged expressions.
        exprs: Vec<ExprId>,
    },
    /// `{@attach expr}`.
    Attach {
        /// The attachment expression.
        expr: ExprId,
    },
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
