//! The NARROW client-plan VOCABULARY — the closed type set the client emitter
//! consumes (extracted from `client_plan.rs` to keep it under the file-size guard).
//!
//! Every supported broad-IR node / attribute / op projects to exactly one of these
//! narrow variants in `client_plan.rs`'s `SupportedClientIr::build`; no broad-IR
//! variant reaches emission. These are pure data definitions — the projection logic
//! and the `ClientModulePlan` / `SupportedClientIr` builder stay in `client_plan.rs`.

use verter_span::Span;

use super::client_allowlist::SupportedHtmlElement;
use super::client_shapes::{ClientBindShape, ClientEventHandlerShape};
use super::ir::{EventOrigin, ExprId, LetBinding, TemplateScopeId};

/// A typed module-scope USER import the client module hoists ABOVE the component
/// function — the general prelude/import carrier (`ClientModulePlan.user_imports`).
///
/// This is the SHARED carrier the broader script-import prelude (arbitrary named /
/// namespace / side-effect imports and module-script items, not yet supported) will
/// broaden, NOT a component-only side channel. The admitted set is exactly the FIRST
/// variant — a default import of a `.svelte` component module — and every other import
/// form stays fail-closed at the classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum UserImport {
    /// `import <local> from '<source>'` where `<source>` resolves to a `.svelte`
    /// component module — the component callee for a `<Local … />` invocation. Emitted
    /// in SOURCE ORDER immediately after the runtime namespace import (`import * as $`).
    ComponentDefault {
        /// The imported local binding name (the component callee, e.g. `Child`).
        local: String,
        /// The module specifier string (`'./Child.svelte'`), emitted verbatim.
        source: String,
        /// The import declaration's source span.
        span: Span,
    },
}

/// A node in the NARROW client node arena — the closed template-node vocabulary
/// the emitter walks. Every supported [`IrNode`] projects to exactly one of these;
/// the broad-IR variants (component / block / tag / non-options special) never
/// reach the plan (they were refused by the classifier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientNode {
    /// A literal text run.
    Text {
        /// The source span.
        span: Span,
        /// The text content.
        text: String,
    },
    /// An HTML comment.
    Comment {
        /// The source span.
        span: Span,
        /// The comment text.
        text: String,
    },
    /// A reactive escaped interpolation (`{expr}`). The reactivity decision was
    /// made at build time (a non-reactive interpolation fails closed before the
    /// plan is built), so every `ReactiveText` node in the plan IS reactive.
    ReactiveText {
        /// The source span.
        span: Span,
        /// The interpolated expression id (into the IR expression arena; the plan
        /// reads it back through the build-time analysis for the op rewrite).
        expr: ExprId,
    },
    /// An intrinsic element. The element is a TYPED [`SupportedHtmlElement`] fact (the
    /// classifier's `try_from` proof), so the emitter reads the DOM var stem from
    /// [`SupportedHtmlElement::var_stem`] — never the raw tag string. The `tag` is
    /// retained for the template SERIALIZATION + the whitespace-context namespace
    /// decision (`for_children_of`), which are HTML-tag concerns, not var stems.
    Element {
        /// The typed accepted element (the SOLE source of the DOM var stem).
        element: SupportedHtmlElement,
        /// The tag name (for serialization + child-namespace context only).
        tag: String,
        /// The full open-tag source span.
        span: Span,
        /// The narrow attributes.
        attrs: Vec<ClientAttr>,
        /// The child node ids (into the plan's narrow node arena).
        children: Vec<ClientNodeId>,
    },
    /// A `{@html expr}` raw-markup insertion node. The raw-markup expression was
    /// REWRITTEN at build time (the fallible rewrite — an `await` / destructuring write
    /// inside the payload fails closed BEFORE the plan exists), so the emitter consumes
    /// the already-rewritten payload from the corresponding [`ClientRuntimeOp::Html`].
    /// The node carries the expression id so the DOM walk can reach the `<!>` anchor it
    /// occupies (or recognise it as the controlled sole child of its parent element).
    RawHtml {
        /// The source span.
        span: Span,
        /// The raw-markup expression id (into the IR expression arena).
        expr: ExprId,
    },
    /// The `<svelte:options>` compile-option marker — consumed, renders nothing
    /// (carried so the node arena mirrors the IR node-id space; the walk skips it).
    OptionsMarker {
        /// The source span.
        span: Span,
    },
    /// A GLOBAL-host special (`<svelte:window|document|body>`) — a NON-RENDERING init-only
    /// host. It clones no template, emits no `$.from_html` / `$.comment` / `$.append`; its
    /// event + bind ops ride the region's [`ClientRuntimeOp`]s (events against `$.window` /
    /// `$.document` / `$.document.body`, binds via the host-expr routing). Carried so the
    /// node arena mirrors the IR node-id space; the DOM walk skips it (it is excluded from
    /// the static skeleton by `is_non_body_special`).
    SpecialHost {
        /// The host kind (`Window` / `Document` / `Body`).
        kind: super::ir::SpecialKind,
        /// The source span.
        span: Span,
    },
    /// A `{#snippet}` DECLARATION marker — non-rendering (dropped from the DOM walk); the
    /// snippet const is emitted separately (module / instance / a component's wrapping
    /// block) by `emit_snippet_decl` reading the IR node. Carried so the node arena mirrors
    /// the IR node-id space.
    SnippetDecl {
        /// The source span.
        span: Span,
    },
    /// A control-flow block (`{#if}`/`{#each}`/`{#await}`/`{#key}`). The head
    /// expressions are REWRITTEN at projection time (the fallible rewrite — an `await`
    /// expression / async rune in a head fails closed BEFORE the plan exists), so the
    /// emitter synthesizes the `$.if`/`$.each`/`$.await`/`$.key` call from this typed
    /// node and recursively emits the child region(s) by their [`TemplateScopeId`].
    Block(ClientBlock),
    /// A run of block-local declaration tags (`{@const}` derived + `{const …}`/`{let
    /// …}` declarations) — emitted as HOISTED statements at the TOP of the region body
    /// (before the clone frame), matching the official `state.consts` placement.
    Declarations {
        /// The declarations in source order.
        decls: Vec<ClientDeclaration>,
    },
    /// A `{@debug a, b}` reactive snapshot-logging effect — emitted at the node's walk
    /// position as `$.template_effect(() => { console.log({ … }); debugger; })`.
    Debug {
        /// One `{ key: $.snapshot(arg) }` entry per debug identifier, in source order.
        entries: Vec<ClientDebugEntry>,
    },
    /// A component invocation (`<Foo …/>` / `<svelte:component>` / `<svelte:self>`) —
    /// the projected `Child(<anchor>, { … })` call (its props rewritten, slot regions
    /// carried by scope id). Emitted directly against the region anchor (a sole-root
    /// STANDALONE component) or against a walked `<!>` node var (a sibling).
    Component(ClientComponent),
    /// A `{@render}` tag — a static snippet call (`pair(node, () => 1)`) or a dynamic
    /// `$.snippet(node, () => fn, …)`, its callee + argument thunks rewritten.
    Render(ClientRender),
    /// A `<svelte:element this={…}>` dynamic element — the comment-anchored `$.element(node,
    /// get_tag, is_svg, callback)` renderable. Its attributes fold into the callback's
    /// `$.attribute_effect` and its binds run against the `$$element` callback param; its
    /// children are the callback's body region.
    SvelteElement(ClientSvelteElement),
    /// A `<svelte:boundary>` error boundary — the comment-anchored `$.boundary(node, { onerror,
    /// failed, pending }, ($$anchor) => { <body> })` renderable. The `failed` / `pending`
    /// `{#snippet}`s hoist to `const`s in a wrapping block above the call (passed by name in
    /// the props); the body is the callback's region.
    Boundary(ClientBoundary),
    /// A `<svelte:head>` — the `$.head('<hash>', ($$anchor) => { <body> })` head-region call.
    /// The `<title>` child (when present) is the `$.document.title = <rhs>` effect emitted in
    /// the callback's after_update slot (between the body-region ops and its `$.append`); the
    /// non-title children (`<meta>` / `<link>` / …) are the body region. The head is EXCLUDED
    /// from the enclosing body skeleton (`is_non_body_special`) and emits its `$.head(...)` at
    /// its source position (interleaved during the walk, like a `{@debug}`).
    Head(ClientHead),
}

/// A projected `<svelte:head>` — the `$.head('<hash>', ($$anchor) => { <body> })` call. The
/// `hash` is the official `hash(filename)` (djb2-XOR, structural); `title` is the optional
/// `$.document.title` effect (the callback's after_update); `body_region` is the non-title
/// head content (`<meta>` / `<link>` / …) rendered inside the callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientHead {
    /// The `$.head('<hash>', …)` scope hash — the official `hash(filename)` over the compile
    /// filename.
    pub(super) hash: String,
    /// The `<title>` effect — `None` for a head with no `<title>`. Emitted in the callback's
    /// after_update slot (between the body-region ops and its `$.append`).
    pub(super) title: Option<ClientTitleEffect>,
    /// The non-title head content region — the `$.from_html` + `$.append` DOM rendered inside
    /// the `($$anchor) => { … }` callback (empty for a title-only head).
    pub(super) body_region: TemplateScopeId,
}

/// A `<svelte:head>`'s `<title>` → `$.document.title = <rhs>` effect. `deferred` picks the
/// wrapper: `has_state` false ⇒ `$.effect(() => { … })` (a static / constant-foldable title),
/// `has_state` true ⇒ `$.deferred_template_effect(…)` (a stateful / call-bearing title). `rhs`
/// is the fully-built assignment right-hand side (a literal, a `value ?? ''`, or a template
/// literal), driven from the typed IR (the official `TitleElement` + `build_template_chunk`).
/// `deps` are the memoized `has_call` chunk bodies (the official `Memoizer.sync_values()`): a
/// NON-empty `deps` emits the deps-array form `$.deferred_template_effect(($0, …) => { … },
/// [() => dep0, …])` (the `rhs` reads the `$0 … $N-1` placeholders); an empty `deps` emits the
/// plain no-param form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientTitleEffect {
    /// Whether the title references state OR memoizes a call (⇒ `$.deferred_template_effect`,
    /// not `$.effect`).
    pub(super) deferred: bool,
    /// The `$.document.title = <rhs>` right-hand side (already rewritten + coalesced; reads the
    /// `$N` placeholders for memoized chunks).
    pub(super) rhs: String,
    /// The memoized `has_call` chunk bodies in `$0 … $N-1` order — the second `[() => …]`
    /// argument of the `$.deferred_template_effect` deps-array form. Empty for a title with no
    /// call chunks.
    pub(super) deps: Vec<String>,
}

/// A projected `<svelte:boundary>` — the comment-anchored `$.boundary(node, props, ($$anchor)
/// => { <body> })` renderable. The `onerror` / `failed` / `pending` ATTRIBUTE props ride the
/// props object (getter or plain init per state-bearing-ness); ALL of the boundary's snippet
/// defs hoist to `const`s in a wrapping `{ … }` block above the call (when present), and only
/// the `failed` / `pending` ones are referenced by name (object shorthand) in the props object
/// AFTER the attribute props.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientBoundary {
    /// The `onerror` / `failed` / `pending` ATTRIBUTE props, in SOURCE order — official's single
    /// attribute loop (`SvelteBoundary.js`). Each is emitted as a getter accessor (`get name() {
    /// return <expr>; }`) when its value is state-bearing, else a plain init (`name: <expr>`).
    /// Empty for a boundary with no attributes.
    pub(super) attr_props: Vec<BoundaryAttrProp>,
    /// ALL of the boundary's `{#snippet}` def node ids, in source order — each hoisted to a
    /// `const <name> = …;` in the wrapping block; only the `failed` / `pending` ones are ALSO
    /// passed by NAME (object shorthand) in the props AFTER the attribute props (filtered at
    /// emit). Empty for a boundary with no snippet children (no wrapping block).
    pub(super) snippets: Vec<super::ir::NodeId>,
    /// The boundary body region — the `($$anchor) => { <body> }` callback's content.
    pub(super) body_region: TemplateScopeId,
}

/// One `<svelte:boundary>` ATTRIBUTE prop — an `onerror` / `failed` / `pending` attribute whose
/// value is an EXPRESSION. Official's `SvelteBoundary.js` attribute loop emits each as
/// `chunk.metadata.expression.has_state ? b.get(name, [b.return(expr)]) : b.init(name, expr)`:
/// a state-bearing value (a prop / signal / snippet reference) becomes the getter accessor `get
/// name() { return <expr>; }`, a constant value (e.g. an inline `onerror` arrow whose only reads
/// are inside its body) becomes the plain `name: <expr>` init.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoundaryAttrProp {
    /// The prop KEY — the source attribute name (`onerror` / `failed` / `pending`).
    pub(super) name: String,
    /// The rewritten value expression — the getter body (`return <expr>;`) or the init value.
    pub(super) expr: String,
    /// Whether the value is STATE-BEARING (⇒ getter accessor) vs a constant (⇒ plain init) —
    /// official's `metadata.expression.has_state` (the sync-only, snippet-name-aware predicate).
    pub(super) has_state: bool,
}

/// A projected `<svelte:element this={…}>` — the comment-anchored `$.element(node, () =>
/// <tag>, <is_svg>, ($$element, $$anchor) => { … })` renderable. The callback body is the
/// element's attribute fold (`$.attribute_effect`) + its `$$element`-hosted binds, then the
/// child-content body region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientSvelteElement {
    /// The get-tag thunk BODY (the `() => <tag>` body): a DYNAMIC tag's rewritten `this={…}`
    /// expression (`tag` / `$$props.tag`), or a STATIC tag's single-quoted literal (`'div'`).
    pub(super) get_tag: String,
    /// Whether the host is an SVG / MathML namespace element (the `$.element` 3rd arg). Always
    /// `false` for the reachable HTML surface (SVG/MathML host elements are not in the client
    /// element allowlist), so an SVG-hosted dynamic element is unreachable until those land.
    pub(super) is_svg: bool,
    /// The LONE-class `$.set_class($$element, 0, …)` fast-path pieces — the official
    /// `SvelteElement` `attributes.length === 1 && class && is_text_attribute` route
    /// (counting the analyze-phase directive-synthesized empty `class`), WITH every
    /// co-located `class:` directive merged into the directive-object argument (the
    /// official `build_set_class`). `Some` only when the SetClass route of
    /// [`svelte_element_attr_route`] fires (the class + `class:` directives then do NOT
    /// appear in `fold`); every other class shape (class + another plain attr, dynamic
    /// class, a co-located `style:` directive) stays in `fold` as plain
    /// `$.attribute_effect` entries. Produced by the SHARED class projection
    /// ([`SetClassPieces`]) — the same substrate the regular-element coalesced class op
    /// uses.
    ///
    /// [`svelte_element_attr_route`]: super::client_svelte_element::svelte_element_attr_route
    pub(super) set_class: Option<SetClassPieces>,
    /// The element's attribute-fold items, in SOURCE ORDER — the entries of the single
    /// `$.attribute_effect($$element, () => ({ … }))` the callback emits. Empty when the
    /// element has no foldable attributes (no `$.attribute_effect` is emitted).
    pub(super) fold: Vec<ElementFoldItem>,
    /// The element's `bind:` directives, run against the `$$element` callback param (each
    /// carries its accepted shape + the rewritten getter/setter, the proxied host setter).
    pub(super) binds: Vec<ClientElementBind>,
    /// The element's LEGACY `on:` listeners, in source order — each a fully-rendered
    /// `$.event('<type>', $$element, <wrapped-handler>[, <capture>][, <passive>])` statement
    /// (the official `SvelteElement` `OnDirective` → `after_update` direct-event path). A MODERN
    /// `on*` attribute (`onclick={…}`) is NOT here — it folds into `$.attribute_effect` via
    /// `fold`. Empty for an element with no legacy `on:` directives.
    pub(super) events: Vec<String>,
    /// The child-content body region — the callback's `($$element, $$anchor) => { … }` body.
    pub(super) body_region: TemplateScopeId,
}

/// One source-ordered entry of a `<svelte:element>`'s `$.attribute_effect` fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ElementFoldItem {
    /// A pre-built fold entry (`class: cls`, `...spread`, `[$.CLASS]: { … }`) — emitted
    /// verbatim into the object literal.
    Entry(String),
    /// An EVENT handler — hoisted to a stable `var <name> = <handler>;` local in the callback
    /// (the official attribute-effect handler-stability hoist), then referenced by name in the
    /// fold (`onclick: <name>`). The hoist name is minted at emit time (collision-safe).
    Event {
        /// The fold object KEY (`onclick`).
        prop: String,
        /// The rewritten handler body (`() => $.update(n)`) — the `var <name> = …;` RHS.
        handler: String,
    },
}

/// The SEMANTIC pieces of one coalesced `$.set_class` write, HOST-INDEPENDENT — the
/// structured base value, the `css_hash` / directive-object / reactivity facts. Produced
/// by the shared class projection (`project_set_class_pieces`) for BOTH the
/// regular-element coalesced class op ([`ClientRuntimeOp::SetClass`], which adds the
/// target node) and the `<svelte:element>` lone-class fast path (which assembles against
/// the `$$element` callback param with `is_html = 0`) — one merge substrate, no per-host
/// class engine. Field semantics match [`ClientRuntimeOp::SetClass`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetClassPieces {
    /// The `value` arg (the base class value) in STRUCTURED form (see the field docs on
    /// the `ClientRuntimeOp::SetClass` op).
    pub(super) value: AttrValue,
    /// The `css_hash` arg (`null` when directives are present, since scoped CSS is
    /// refused upstream), or `None` when there are no directives (omitted).
    pub(super) css_hash: Option<String>,
    /// The directives object `{ foo: cond, … }`, or `None` for a base-only class.
    pub(super) directives: Option<String>,
    /// Whether ANY directive value `has_call` (so the whole directives object arg is
    /// memoized into a `$N` deps-array slot when the op is reactive).
    pub(super) directives_has_call: bool,
    /// Whether the call is REACTIVE — `has_state || base.has_call() ||
    /// directives_has_call` (a stateful OR `has_call` base/directive forces the effect +
    /// memoization, the official rule).
    pub(super) reactive: bool,
    /// The accumulator STEM (`classes`) when the reactive-directive path needs the
    /// `let <name>;` accumulator; `None` otherwise.
    pub(super) accumulator_stem: Option<&'static str>,
}

/// A `<svelte:element>` `bind:` directive run against the `$$element` callback param — the
/// accepted shape plus the rewritten getter/setter (the proxied host setter `$.set(local,
/// $$value, true)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientElementBind {
    /// The accepted bind shape (`This` / a `DomBind` dimension/property routing).
    pub(super) shape: ClientBindShape,
    /// The rewritten getter body.
    pub(super) getter: String,
    /// The rewritten setter body (carries the `$.set(local, $$value, true)` proxy flag).
    pub(super) setter: String,
}

/// A projected component invocation — the closed structural mirror of a `<Foo …/>` /
/// `<svelte:component>` / `<svelte:self>` node, built (with every expression rewritten)
/// in `client_component_plan`. The emitter assembles the `Child(<anchor>, <props>)` call
/// from this typed structure; the slot callbacks are emitted through the shared
/// region-callback emitter at their [`TemplateScopeId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientComponent {
    /// The callee identity (static imported local, `<svelte:self>` recursion, or a
    /// dynamic `<svelte:component this={…}>`).
    pub(super) callee: ComponentCallee,
    /// The full open-tag source span.
    pub(super) span: Span,
    /// The function-pair component binds (`bind:x={get, set}`) — each carries its rewritten
    /// get/set expressions plus a component-function-scoped pair INDEX. The emitter mints the
    /// `var bind_get` / `var bind_set` locals from that index through the shared scope-aware
    /// allocator (so they provably avoid every user binding and each other) and emits them at
    /// the CALL's statement level, BEFORE it (the official `state.init` placement). `var`
    /// hoists, so they sit beside the call without forcing a block.
    pub(super) fn_pair_binds: Vec<ComponentFnPairBind>,
    /// Statements emitted INSIDE the component's `{ … }` wrapping block, before the call —
    /// the prop deriveds (`let $0 = $.derived(() => …)` for a compound reactive prop, the
    /// official `memoizer.deriveds()` placement). A non-empty list forces the block.
    pub(super) block_statements: Vec<String>,
    /// The props payload — a plain `{ … }` object, or a `$.spread_props(…)` call when any
    /// `{...spread}` attribute is present.
    pub(super) props: ComponentProps,
    /// The snippet-def child nodes (the `{#snippet}` blocks declared directly inside the
    /// component) — emitted as local consts in the wrapping `{ … }` block BEFORE the call.
    pub(super) snippet_defs: Vec<super::ir::NodeId>,
    /// `bind:this={ref}` — the (setter, getter) bodies wrapping the call in
    /// `$.bind_this(<call>, <setter>, <getter>)`.
    pub(super) bind_this: Option<ComponentBindThis>,
}

/// A component function-pair `bind:x={get, set}` — the rewritten getter/setter expressions
/// plus the component-function-scoped pair INDEX. The init `var bind_get` / `var bind_set`
/// locals are NOT named here: the emitter mints them from this index through the shared
/// scope-aware name allocator (the same `used`/`alloc_name` rail the DOM-var and `bind:group`
/// accumulator names use), so the minted names provably avoid every user binding AND each
/// other — matching the official per-component-function `scope.generate('bind_get')` uniquing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComponentFnPairBind {
    /// The component-function-scoped pair index (assigned in source order across EVERY
    /// component call in the component function). The emitter's allocator pass keys the
    /// `(bind_get, bind_set)` name pair on it, and the [`ComponentMember::FnPairGetSet`]
    /// member links back to its names through the same index.
    pub(super) index: usize,
    /// The rewritten getter expression (the right-hand side of `var <bind_get> = …;`).
    pub(super) get_expr: String,
    /// The rewritten setter expression (the right-hand side of `var <bind_set> = …;`).
    pub(super) set_expr: String,
}

/// The callee identity of a [`ClientComponent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ComponentCallee {
    /// A static component callee — the imported local (`Child`) or, for
    /// `<svelte:self>`, the component's own compile-name. Emitted `Name(<anchor>, …)`.
    Static {
        /// The callee identifier.
        name: String,
    },
    /// A dynamic `<svelte:component this={expr}>` — emitted `$.component(<anchor>, () =>
    /// <this>, ($$anchor, $$component) => { $$component($$anchor, <props>); })`.
    Dynamic {
        /// The rewritten `this={…}` expression (the `() => <this>` thunk body).
        this_expr: String,
    },
}

/// The props payload of a [`ClientComponent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ComponentProps {
    /// A plain `{ <members> }` object literal (no spread attribute).
    Object(Vec<ComponentMember>),
    /// A `$.spread_props(<parts>)` call — present when ANY `{...spread}` attribute is on
    /// the component. The parts interleave object groups and spread thunks in SOURCE
    /// ORDER (the official `props_and_spreads`).
    Spread(Vec<ComponentSpreadPart>),
}

/// One part of a `$.spread_props(…)` call — an object group of members or a spread
/// thunk, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ComponentSpreadPart {
    /// A `{ <members> }` object group.
    Group(Vec<ComponentMember>),
    /// A spread thunk (`() => $$props.rest`) or bare expression.
    Spread {
        /// The already-rewritten spread argument (a `() => …` thunk, or a bare expr).
        arg: String,
    },
}

/// One member of a component's props object, in emission order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ComponentMember {
    /// `key: value` — a plain init prop (a static literal, a constant-expr dynamic, or a
    /// non-reactive value like a callback `onfoo: () => …`).
    Init {
        /// The prop key.
        key: String,
        /// The already-rewritten value expression.
        value: String,
    },
    /// `get key() { return body; }` — a reactive prop value (a getter so the child
    /// re-reads on change).
    Getter {
        /// The prop key.
        key: String,
        /// The already-rewritten getter body.
        body: String,
    },
    /// `get key() { return get_body; } set key($$value) { set_body; }` — a SIMPLE component
    /// `bind:prop` (the getter/setter bodies are already rewritten from the bound lvalue).
    GetSet {
        /// The bind prop key.
        key: String,
        /// The already-rewritten getter body.
        get_body: String,
        /// The already-rewritten setter body.
        set_body: String,
    },
    /// `get key() { return <bind_get>(); } set key($$value) { <bind_set>($$value); }` — a
    /// component FUNCTION-PAIR `bind:x={get, set}`. The local names are NOT baked here (the
    /// allocator has not run at projection time); the emitter resolves the `(bind_get,
    /// bind_set)` pair from `index` (minted through the shared scope-aware allocator), so they
    /// never collide with a user binding.
    FnPairGetSet {
        /// The bind prop key.
        key: String,
        /// The component-function-scoped pair index (links to the owning
        /// [`ComponentFnPairBind`] and its allocator-minted names).
        index: usize,
    },
    /// `$$events: { <entries> }` — the `on:`-directive handlers forwarded to the component.
    Events {
        /// The `(event-type, handler)` entries in source order. The emitter routes each key
        /// through [`object_key`](super::client_codegen_helpers::object_key), so a hyphenated
        /// `on:foo-bar` quotes to `'foo-bar': …` (a bare `foo-bar:` is unparseable JS).
        entries: Vec<(String, String)>,
    },
    /// A `{#snippet name}`-as-child shorthand prop (`name` — the object shorthand
    /// `name: name`), the runes named-slot-via-snippet interop.
    SnippetProp {
        /// The snippet name (the shorthand key + value).
        name: String,
    },
    /// `children: ($$anchor, $$slotProps) => { … }` — the default-slot region callback
    /// (no `let:` directive on the component / default fragment).
    DefaultChildren {
        /// The default-slot region.
        region: TemplateScopeId,
    },
    /// `children: $.invalid_default_snippet` — the sentinel emitted when the default slot
    /// carries `let:` slot props (so the real default slot rides `$$slots.default`).
    InvalidDefaultSnippet,
    /// `$$slots: { <entries> }` — the slots object (named-slot callbacks, `name: true`
    /// snippet/default markers, and the `let:`-default callback).
    Slots {
        /// The slot entries, in source order.
        entries: Vec<SlotEntry>,
    },
}

/// One entry of a component's `$$slots` object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SlotEntry {
    /// `name: true` — a marker for a `{#snippet}`-as-prop or a `children`-prop default
    /// slot (so the child's `<slot name>` resolves).
    TrueMarker {
        /// The slot name (`default` / `header` / …).
        name: String,
    },
    /// `name: ($$anchor, $$slotProps) => { … }` — a named-slot region callback (a
    /// `<svelte:fragment slot>` / `slot=`-bearing child) or the `let:`-default callback.
    Callback {
        /// The slot name.
        name: String,
        /// The slot-content region.
        region: TemplateScopeId,
        /// The slot's `let:` slot-prop bindings, prepended to the callback body as
        /// `const <name> = $.derived(() => $$slotProps.<key>);` — PLANNED here (from the
        /// component's `default_lets` / the named slot's `lets`) so the emitter consumes the
        /// fact directly and never rescans the IR / binding table per slot closure.
        lets: Vec<LetBinding>,
    },
}

/// A component `bind:this={ref}` — the (setter, getter) bodies the emitter wraps the
/// component call in (`$.bind_this(<call>, <setter>, <getter>)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComponentBindThis {
    /// The setter body (`($$value) => ref = $$value`).
    pub(super) setter: String,
    /// The getter body (`() => ref`).
    pub(super) getter: String,
}

/// A projected `{@render}` tag — a static snippet call or a dynamic `$.snippet`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientRender {
    /// Whether the callee is DYNAMIC (`$.snippet(node, () => fn, …args)`) vs a STATIC
    /// snippet-name call (`name(node, …args)`).
    pub(super) dynamic: bool,
    /// The rewritten callee — a static snippet name (`pair`), or the dynamic snippet
    /// function thunk body (`$$props.children ?? $.noop` / `cond ? $$props.a : $$props.b`).
    pub(super) callee: String,
    /// Whether a STATIC callee is a direct call (`pair(node, …)`) vs a `$.maybe_call`
    /// (`{@render maybeSnippet?.()}` — but a resolved static snippet is always a direct
    /// call, so this is `false` in the supported surface).
    pub(super) maybe_call: bool,
    /// The rewritten argument thunks (`() => 1`, `() => $.get(n)`), in source order.
    pub(super) args: Vec<String>,
}

/// The reactive ops for ONE template-scope region, in source order. A block body is
/// its OWN region with its OWN ops: the emitter emits each region's combined
/// `$.template_effect` + binds + events from its region's ops, NOT the root's. The op
/// expressions were already rewritten in each op's OWN recorded scope (so a body op
/// reads `$.get(item)` while a root op reads the root signal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RegionOps {
    /// The owning template scope.
    pub(super) scope_id: TemplateScopeId,
    /// The region's reactive ops, in source order.
    pub(super) ops: Vec<ClientRuntimeOp>,
}

/// A control-flow block with its head expressions rewritten + child-region scope ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientBlock {
    /// `{#if}` chain — branches in source order; the trailing `test: None` branch is
    /// the `{:else}`.
    If {
        /// The if/else-if/else branches, in source order.
        branches: Vec<ClientIfBranch>,
    },
    /// `{#each}` — keyed/unkeyed, optional index, optional `{:else}`.
    Each(ClientEach),
    /// `{#await}` — pending/then/catch.
    Await(ClientAwait),
    /// `{#key expr}` — `$.key(node, () => expr, ($$anchor) => { … })`.
    Key {
        /// The rewritten key expression (the `() => expr` thunk body).
        expr: String,
        /// The body region.
        body: TemplateScopeId,
    },
}

/// One branch of an `{#if}` chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientIfBranch {
    /// The rewritten branch test, or `None` for the `{:else}` branch.
    pub(super) test: Option<String>,
    /// The branch body region.
    pub(super) body: TemplateScopeId,
}

/// A projected `{#each}` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientEach {
    /// The official EACH flags bitmask (`EACH_ITEM_REACTIVE` | `EACH_INDEX_REACTIVE` |
    /// `EACH_IS_CONTROLLED` | `EACH_ITEM_IMMUTABLE`).
    pub(super) flags: u8,
    /// The rewritten source expression (the `() => SOURCE` thunk body).
    pub(super) source: String,
    /// The KEY callback for a keyed each (`(item) => key`), or `None` for an unkeyed
    /// each (emitted as the `$.index` literal).
    pub(super) key: Option<ClientEachKey>,
    /// The item binding param name (`None` for the no-item `{#each {length}}` form).
    pub(super) item_param: Option<String>,
    /// The index binding param name, emitted ONLY when [`ClientEach::emit_index`] is set.
    pub(super) index_param: Option<String>,
    /// Whether the index render param is emitted (the official `uses_index` rule: the
    /// index is read, OR the item is reassigned / mutated).
    pub(super) emit_index: bool,
    /// The body region.
    pub(super) body: TemplateScopeId,
    /// The `{:else}` fallback region.
    pub(super) else_body: Option<TemplateScopeId>,
}

/// The key callback of a keyed `{#each}` — emitted in its OWN callback scope (the key
/// expression is PLAIN, never body-signal-rewritten).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientEachKey {
    /// The key-callback params (`(item)` or `(item, index)` when the key reads the index).
    pub(super) params: Vec<String>,
    /// The key expression rewritten in the KEY scope (plain, NOT body-signal-rewritten).
    pub(super) expr: String,
}

/// A projected `{#await}` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientAwait {
    /// The rewritten promise expression (the `() => PROMISE` thunk body).
    pub(super) promise: String,
    /// The pending body region (`None` → the `null` argument slot).
    pub(super) pending: Option<TemplateScopeId>,
    /// The `{:then v}` value param name.
    pub(super) then_param: Option<String>,
    /// The `{:then}` body region.
    pub(super) then_body: Option<TemplateScopeId>,
    /// The `{:catch e}` error param name.
    pub(super) catch_param: Option<String>,
    /// The `{:catch}` body region.
    pub(super) catch_body: Option<TemplateScopeId>,
}

/// One block-local declaration (a `{@const}` derived memo, a `{const}/{let}` inert
/// declarator, or a rune-carrying `{let x = $state(…)}` declarator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientDeclaration {
    /// `{@const x = INIT}` (runes mode) → `const x = $.derived(() => INIT);`.
    Derived {
        /// The declared name.
        name: String,
        /// The rewritten initializer (the `() => INIT` derived body).
        init: String,
    },
    /// `{const x = INIT}` / `{let x = INIT}` / `{let x}` inert declarator → a plain
    /// block-local `const`/`let` (NO `$.derived`, NO `$.get`); the initializer is
    /// signal-rewritten but the binding itself is inert.
    Inert {
        /// The declaration keyword.
        keyword: ClientDeclKeyword,
        /// The declared name.
        name: String,
        /// The rewritten initializer, or `None` for a bare `let x;`.
        init: Option<String>,
    },
    /// A rune-carrying `{let x = $state(…)}` / `{let x = $derived(…)}` declarator,
    /// classified through the instance-script rune/state pipeline → the already-lowered
    /// declaration statement (`let x = $.state(…)` / `let x = $.derived(…)`).
    Rune {
        /// The fully-lowered declaration statement (without trailing `;`).
        code: String,
    },
}

/// The declaration keyword of an inert `{const}/{let}` declarator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientDeclKeyword {
    /// `const`.
    Const,
    /// `let`.
    Let,
}

/// One `{ key: $.snapshot(arg) }` entry of a `{@debug}` effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientDebugEntry {
    /// The object key (the debug identifier name).
    pub(super) key: String,
    /// The rewritten `$.snapshot(<expr>)` argument expression.
    pub(super) snapshot_arg: String,
}

/// A node id into the plan's narrow node arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ClientNodeId(pub(super) u32);

/// A narrow supported attribute on a [`ClientNode::Element`]. The bind / event
/// REWRITES live on the [`ClientRuntimeOp`]s (the emitter sequences ops, not
/// element attrs); the element attr records the supported KIND so the narrow node
/// tree is a faithful structural mirror. The static attribute carries its literal
/// (folded into the template HTML by the planner).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientAttr {
    /// A truly-static attribute (folded into the static template HTML).
    Static {
        /// The attribute name.
        name: String,
        /// The literal value (`None` for a valueless boolean attribute).
        value: Option<String>,
    },
    /// A `bind:` directive on the element (a DOM value/property bind or element
    /// `bind:this`) — the helper-routing + getter/setter rewrite is on the
    /// corresponding [`ClientRuntimeOp::Bind`] (carried by its accepted
    /// [`ClientBindShape`]). This narrow attr records the coarse target KIND only (the
    /// node tree stays a faithful structural mirror).
    Bind {
        /// The coarse bind target kind (`this` vs a DOM value/property bind).
        target: ClientBindTarget,
    },
    /// A DOM event registration — the precise emission (mode / capture / passive /
    /// modifier-wrapper stack / rewritten handler) lives on the corresponding
    /// [`ClientRuntimeOp::Event`]'s [`EventEmit`]. This narrow attr records the
    /// coarse KIND only (the event type + the delegated-vs-direct mode), so the node
    /// tree stays a faithful structural mirror — mirroring [`ClientAttr::Bind`]'s
    /// coarse-target-kind discriminant.
    Event {
        /// The normalized event type (`click`, `focus`, …).
        event_type: String,
        /// The coarse registration mode (delegated `$.delegated` vs direct `$.event`).
        mode: EventMode,
    },
    /// A dynamic attribute / `class` / `style` surface — the emission
    /// (`$.set_attribute` / a property write / `$.set_class` / `$.set_style` /
    /// `$.autofocus`) is on the corresponding [`ClientRuntimeOp`]. The element attr
    /// records the supported KIND only (the narrow node tree stays a faithful
    /// structural mirror).
    Dynamic,
    /// An element LIFECYCLE directive (`use:` / `transition:`/`in:`/`out:` /
    /// `animate:` / element-position `{@attach}`) — the emission (`$.action` /
    /// `$.transition` / `$.animation` / `$.attach`, with the FLAG arithmetic and
    /// thunk shapes) lives on the corresponding
    /// [`ClientRuntimeOp::Lifecycle`]'s [`ElementLifecycleOp`]. The element attr
    /// records the supported KIND only.
    Lifecycle,
}

/// The COARSE supported bind target kind — the structural-mirror discriminant on
/// [`ClientAttr::Bind`]. The PRECISE helper routing (which `$.bind_*` / `bind_property`
/// form, arity, event, prelude) lives on the op's accepted [`ClientBindShape`]
/// ([`ClientBindShape::DomBind`] carries the typed `RuntimeBindRouting`); this enum
/// only distinguishes the two emission FAMILIES (the render-side `bind:this` vs a
/// DOM value/property bind), which differ in WHEN they are emitted (inline-in-walk vs
/// post-walk).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientBindTarget {
    /// A DOM value/property bind (`value`/`checked`/`group`/media/dimension/
    /// contenteditable/property) — emitted post-walk, routed by its
    /// [`ClientBindShape::DomBind`] `RuntimeBindRouting`.
    DomValue,
    /// `bind:this` — a render-side binding emitted INLINE during the walk.
    This,
}

/// The reusable event-emission substrate carried on a planned [`ClientRuntimeOp::Event`].
///
/// This is the typed representation the official `$.event` / `$.delegated` emit shape
/// is driven from — the emitter never re-infers a decision from source text. The
/// regular-element event surface only PRODUCES `EventEmitTarget::Node` hosts, but the
/// type models every target kind so the SAME emitter serves the special-element event
/// hosts (`<svelte:window|body|document>`) by feeding the non-`Node` targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EventEmit {
    /// The registration mode (delegated `$.delegated` vs direct `$.event`).
    pub mode: EventMode,
    /// Which authored syntax produced this event (modern `on*` attribute vs legacy
    /// `on:` directive) — the discriminant the EMISSION SLOT keys on
    /// ([`super::client_lifecycle::event_emission_slot`]): a bare LEGACY `on:` event
    /// on a regular node joins the post-walk directive batch (source-ordered with
    /// `$.transition` / `$.animation`) and effect-wraps into the init domain on a
    /// `use:` action host; a MODERN event emits in the pre-batch post-walk phase and
    /// never wraps. Delegation (`mode`) is NOT the wrap/order discriminant.
    pub origin: EventOrigin,
    /// The registration target host (the regular-element surface emits only `Node`).
    pub target: EventEmitTarget,
    /// The normalized event type (`click`, `focus`, …) — the `$.event` / `$.delegated`
    /// first positional string argument.
    pub event_type: String,
    /// Whether this is a CAPTURE-phase handler — the 4th positional `true` (emitted as
    /// the `void 0` placeholder when absent but a later `passive` arg is present).
    pub capture: bool,
    /// The passive-listener option — the 5th positional boolean: `Some(true)` passive,
    /// `Some(false)` nonpassive, `None` omitted.
    pub passive: Option<bool>,
    /// The legacy modifier wrappers in the FIXED official application order
    /// (inner→outer), each wrapping the previous handler (`$.<modifier>(handler)`).
    pub wrappers: Vec<EventWrapper>,
    /// The rewritten handler body (the innermost expression the wrappers nest).
    pub handler: String,
}

/// The event-registration mode — the `$.delegated` (document-level delegation) vs
/// `$.event` (direct per-node listener) helper choice. A legacy `on:` directive and a
/// capture / non-bubbling event are ALWAYS direct; only a modern bubbling-event
/// attribute on a regular element delegates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventMode {
    /// A direct `$.event(...)` per-node listener.
    Direct,
    /// A delegated `$.delegated(...)` listener (registered in the module
    /// `$.delegate([...])` epilogue).
    Delegated,
}

/// The event-registration target host. The regular-element surface produces only
/// `Node`; the global hosts are the reusable substrate the special-element event hosts
/// consume (the special-element NODE gate stays closed for regular elements, so the
/// global variants are never produced on that path).
///
/// `dead_code` is allowed for the global-host variants: they are NOT dead — the emitter
/// (`event_target_host`) resolves all four to their host expression and the
/// `event_target_host_resolves_node_and_global_hosts` test exercises every variant — but
/// the regular-element surface never CONSTRUCTS them (the special-element event hosts
/// do), so the non-test lib build sees them as unconstructed. Carrying them typed lets
/// the special-element event hosts reuse the SAME emitter without re-extending the enum.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventEmitTarget {
    /// A DOM node in the template (the var resolved from the node arena).
    Node(ClientNodeId),
    /// The `window` global (`$.window`) — a `<svelte:window>` listener.
    Window,
    /// The `document` global (`$.document`) — a `<svelte:document>` listener.
    Document,
    /// The document `body` (`$.document.body`) — a `<svelte:body>` listener.
    Body,
    /// The `<svelte:element>` callback's element param (`$$element`) — a LEGACY `on:`
    /// directive on a dynamic element emits a DIRECT `$.event('type', $$element, …)` in the
    /// element callback body (the official `SvelteElement` `OnDirective` → `after_update`
    /// path), NOT an `$.attribute_effect` fold entry (that is the MODERN `on*` attribute form).
    SvelteElement,
}

/// A legacy `on:` event modifier WRAPPER — each wraps the handler in its official
/// `svelte/internal/client` helper (`$.<modifier>(handler)`). The wrappers apply in
/// the FIXED order [`EventWrapper::ORDER`] (inner→outer), INDEPENDENT of source order
/// — matching the official `OnDirective.js` modifier iteration. `capture` / `passive`
/// / `nonpassive` are NOT wrappers (they are positional `$.event` args) and have no
/// variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventWrapper {
    /// `$.stopPropagation(handler)`.
    StopPropagation,
    /// `$.stopImmediatePropagation(handler)`.
    StopImmediatePropagation,
    /// `$.preventDefault(handler)`.
    PreventDefault,
    /// `$.self(handler)`.
    SelfTarget,
    /// `$.trusted(handler)`.
    Trusted,
    /// `$.once(handler)` — a per-instance-closure once wrapper, NOT `{ once: true }`.
    Once,
}

impl EventWrapper {
    /// The FIXED official application order (inner→outer): the `OnDirective.js`
    /// modifier iteration order. `stopPropagation` is the INNERMOST wrapper (applied
    /// first, closest to the handler), `once` the OUTERMOST.
    pub(super) const ORDER: [EventWrapper; 6] = [
        EventWrapper::StopPropagation,
        EventWrapper::StopImmediatePropagation,
        EventWrapper::PreventDefault,
        EventWrapper::SelfTarget,
        EventWrapper::Trusted,
        EventWrapper::Once,
    ];

    /// The legacy `on:` modifier NAME this wrapper is produced from (`stopPropagation`,
    /// `preventDefault`, …) — the typed mapping from a parsed modifier string.
    pub(super) fn from_modifier(name: &str) -> Option<EventWrapper> {
        match name {
            "stopPropagation" => Some(EventWrapper::StopPropagation),
            "stopImmediatePropagation" => Some(EventWrapper::StopImmediatePropagation),
            "preventDefault" => Some(EventWrapper::PreventDefault),
            "self" => Some(EventWrapper::SelfTarget),
            "trusted" => Some(EventWrapper::Trusted),
            "once" => Some(EventWrapper::Once),
            _ => None,
        }
    }

    /// The `svelte/internal/client` helper member name (`$.<helper>`) this wrapper
    /// emits — matching the official helper identity EXACTLY.
    pub(super) fn helper(self) -> &'static str {
        match self {
            EventWrapper::StopPropagation => "stopPropagation",
            EventWrapper::StopImmediatePropagation => "stopImmediatePropagation",
            EventWrapper::PreventDefault => "preventDefault",
            EventWrapper::SelfTarget => "self",
            EventWrapper::Trusted => "trusted",
            EventWrapper::Once => "once",
        }
    }
}

/// A narrow supported script item — a single emitted component-FUNCTION-BODY
/// statement (already lowered to its final client JS text). The supported instance
/// script is the strict finite [`SupportedInstanceScriptItem`](super::client_shapes::SupportedInstanceScriptItem)
/// allowlist; a `<script module>` / instance `import` / `export` is fail-closed
/// upstream (the script-hoisting deferral), so the closed body vocabulary is a single
/// `BodyStatement` variant — the plan carries the emitted string, the emitter only
/// sequences it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientScriptItem {
    /// An emitted component-FUNCTION-BODY statement (a supported `$state` declaration
    /// or a `bind:this` clone-root local) — already lowered to its final client JS
    /// text.
    BodyStatement {
        /// The emitted statement.
        code: String,
    },
}

impl ClientScriptItem {
    /// The emitted statement text for this script item.
    pub(super) fn code(&self) -> &str {
        match self {
            Self::BodyStatement { code } => code,
        }
    }
}

/// A narrow supported reactive runtime op — the closed op vocabulary the emitter
/// consumes. Every supported [`RuntimeOp`] projects to one of these (with its
/// expressions already rewritten); the broad-op variants never reach the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientRuntimeOp {
    /// Reactive text content for an interpolation's text node. The interpolated
    /// expression was REWRITTEN at build time (the fallible rewrite — an `await` /
    /// destructuring write inside an interpolation fails closed BEFORE the plan is
    /// built), so the emit-time memoizer consumes the already-rewritten text.
    ReactiveText {
        /// The target node id (into the plan node arena).
        target: ClientNodeId,
        /// The interpolated expression id (the emit-time text-run partition reads it
        /// back through the build analysis for the mixed-run template assembly).
        expr: ExprId,
        /// The already-rewritten expression text (the value the memoizer routes
        /// inline or hoists into a `$N` placeholder).
        rewritten: String,
        /// Whether the expression `has_call` (drives the memoizer deps-array form).
        has_call: bool,
    },
    /// A `bind:*` op (a DOM value/property bind or element `bind:this`).
    Bind {
        /// The target node id.
        target: ClientNodeId,
        /// The accepted bind SHAPE fact (from the default-deny classifier) — the
        /// typed sub-shape the op was admitted as. For a DOM value/property bind it
        /// carries the precise `RuntimeBindRouting` (helper / arity / event); the
        /// emitter consumes it DATA-DRIVEN, so the op is a typed classification, not
        /// just a rewritten string pair. (The render-side `bind:this` vs post-walk
        /// timing split is read off this shape.)
        shape: ClientBindShape,
        /// The rewritten getter body.
        getter: String,
        /// The rewritten setter body.
        setter: String,
    },
    /// A DOM event registration (`$.event` direct or `$.delegated` delegated).
    Event {
        /// The reusable event-emission metadata — mode / target host / capture /
        /// passive / modifier-wrapper stack / rewritten handler. The emitter consumes
        /// this typed substrate (never a re-inferred emit-time decision), and the SAME
        /// emitter serves the special-element event hosts by feeding the non-`Node`
        /// [`EventEmitTarget`] variants.
        emit: EventEmit,
        /// The accepted handler SHAPE fact (from the default-deny classifier) — the
        /// typed sub-shape the handler was admitted as, carried as the acceptance
        /// record (the emission itself is driven by [`EventEmit`]).
        shape: ClientEventHandlerShape,
    },
    /// A dynamic plain-attribute write — `$.set_attribute(node, 'name',
    /// value)` OR a DOM-property write `node.<prop> = value`, decided by the accepted
    /// [`ClientDynAttrEmit`] shape. The value expression(s) are already rewritten. A
    /// REACTIVE write joins the combined `$.template_effect` (and its `has_call`
    /// expression parts are memoized); a non-reactive write is a plain init statement.
    ReactiveAttr {
        /// The target node id.
        target: ClientNodeId,
        /// The emission shape (set_attribute / property write / autofocus).
        emit: ClientDynAttrEmit,
        /// Whether the value is REACTIVE — `has_state || value.has_call()` (the
        /// official rule: a stateful value joins `state.update`, AND a `has_call`
        /// value is memoized into a `$N` placeholder that only the effect can bind, so
        /// it joins the effect too). A reactive write joins the combined effect; a
        /// non-reactive write is a one-shot init.
        reactive: bool,
    },
    /// A coalesced `$.set_class(node, is_html, value, css_hash, prev, next)` write
    /// — one per element, merging the `class={…}` base with every `class:`
    /// directive. The op carries the SEMANTIC pieces (already rewritten); the emitter
    /// assembles the final call with the real DOM var + accumulator name, so no
    /// post-hoc string substitution is needed.
    SetClass {
        /// The target node id.
        target: ClientNodeId,
        /// The `value` arg (the base class value) in STRUCTURED form — a `$.clsx(...)`
        /// wrap [`AttrValue::Single`], a static string literal [`AttrValue::Const`], a
        /// mixed `` `lit${expr ?? ''}lit` `` template [`AttrValue::Mixed`], or `''`
        /// [`AttrValue::Const`] for a directive-only class. The emitter routes it
        /// through the shared memoizer at emit time, so a mixed base memoizes each
        /// EXPRESSION PART (`` `a${$0 ?? ''}b` ``, dep `() => call`) and a `$.clsx(...)`
        /// base memoizes the whole wrap — the official `build_set_class` rule.
        value: AttrValue,
        /// The `css_hash` arg (`null` when directives are present, since scoped CSS is
        /// refused upstream), or `None` when there are no directives (omitted).
        css_hash: Option<String>,
        /// The directives object `{ foo: cond, … }`, or `None` for a base-only class.
        directives: Option<String>,
        /// Whether ANY directive value `has_call` (so the whole directives object arg
        /// is memoized into a `$N` deps-array slot when the op is reactive).
        directives_has_call: bool,
        /// Whether the call is REACTIVE — `has_state || base.has_call() ||
        /// directives_has_call` (a stateful OR `has_call` base/directive forces the
        /// effect + memoization, the official rule). A reactive call joins the
        /// combined effect; a non-reactive call is a one-shot init.
        reactive: bool,
        /// The accumulator STEM (`classes`) when the reactive-directive path needs the
        /// `let <name>;` accumulator; `None` otherwise. The emitter resolves the stem to
        /// a collision-free name and uses it for BOTH the `prev` arg and the `<name> =`
        /// assignment.
        accumulator_stem: Option<&'static str>,
    },
    /// A coalesced `$.set_style(node, value, prev, next)` write — one per
    /// element, merging the `style={…}` base with every `style:` directive. The op
    /// carries the SEMANTIC pieces (see [`ClientRuntimeOp::SetClass`]).
    SetStyle {
        /// The target node id.
        target: ClientNodeId,
        /// The `value` arg (the base style value) in STRUCTURED form — a dynamic
        /// expression [`AttrValue::Single`], a static string literal / `''` for a
        /// directive-only style [`AttrValue::Const`], or a mixed template
        /// [`AttrValue::Mixed`]. The emitter memoizes each expression part of a mixed
        /// base at emit time (the official `build_set_style` rule).
        value: AttrValue,
        /// The directives object `{ prop: v, … }`, or the `[normal, important]` array
        /// when any `|important` directive is present, or `None` for a base-only style.
        directives: Option<String>,
        /// Whether ANY directive value `has_call` (so the whole directives arg —
        /// object or `[normal, important]` array — is memoized when the op is reactive).
        directives_has_call: bool,
        /// Whether the call is REACTIVE — `has_state || base.has_call() ||
        /// directives_has_call` (a stateful OR `has_call` base/directive forces the
        /// effect + memoization). A reactive call joins the combined effect; a
        /// non-reactive call is a one-shot init.
        reactive: bool,
        /// The accumulator STEM (`styles`) when the reactive-directive path needs one.
        accumulator_stem: Option<&'static str>,
    },
    /// A coalesced `$.attribute_effect(el, () => ({ <fold> }))` write — the SINGLE
    /// reactive effect a spread element gets in place of the per-attribute path. The
    /// presence of ANY spread on an element switches its WHOLE attribute strategy: every
    /// co-located attribute folds into the single object literal the effect returns — plain
    /// attributes (static / dynamic / mixed / a plain `class` / `style` attribute) and the
    /// spreads themselves IN SOURCE ORDER, with every `class:` directive merged into ONE
    /// trailing `[$.CLASS]: { … }` and every `style:` directive into ONE trailing
    /// `[$.STYLE]: { … }` appended LAST (the official `Element.js` spread path) — and the
    /// element emits NO separate `$.set_attribute` / `$.set_class` / `$.set_style` /
    /// property write and NO `$.template_effect`. The op carries the already-assembled object-literal BODY (the
    /// fold text between the `{` and `}`, expressions rewritten through the shared
    /// rewriter); the emitter wraps it in the `el, () => ({ <body> })` call with the real
    /// DOM var.
    AttributeEffect {
        /// The target node id.
        target: ClientNodeId,
        /// The assembled object-literal fold body (`...p, id: x, [$.CLASS]: { on: c }`),
        /// source-ordered. Empty for an attribute-less spread is impossible (a spread is
        /// always present), so the body always carries at least one `...spread`.
        fold_body: String,
        /// Whether the element takes the trailing `void 0, void 0, void 0, void 0, true`
        /// argument tail — the official form for a void / self-closing element (an
        /// `<input>`, whose value/defaultValue handling the trailing `true` flags). A
        /// 2-argument call otherwise.
        input_trailing: bool,
    },
    /// An element LIFECYCLE op (`$.action` / `$.transition` / `$.animation` /
    /// `$.attach`) — the typed [`ElementLifecycleOp`] carries the target node var +
    /// the already-rewritten expression pieces (and, for a transition, the
    /// precomputed FLAG integer). The emitter renders the exact official call shape
    /// from this substrate in TWO phases: `Action` / `Attachment` emit INLINE during
    /// the walk (the init domain — after the element's attr inits, interleaved with
    /// `bind:this` in source order), while `Transition` / `Animation` emit LAST in
    /// the post-walk op stage (after the binds + events) — the official
    /// `RegularElement` phase order.
    Lifecycle(ElementLifecycleOp),
    /// A `{@html node, () => h [, true])` raw-markup insertion. The payload is the
    /// already-assembled SECOND argument — a `() => <rewritten-expr>` thunk, or the bare
    /// callee identifier when the payload is a direct zero-argument identifier call
    /// (`{@html render()}` → `render`, the official thunk elision). `only_child` selects
    /// the topology: a `{@html}` that is the SOLE controlled child of its parent element
    /// operates on the PARENT element var with the trailing `true` argument and is
    /// followed by `$.reset(parent)`; a `{@html}` with siblings operates on its OWN `<!>`
    /// anchor var (reached by the DOM walk) with NO trailing argument.
    Html {
        /// The target node id (the `{@html}` tag node). For the only-child case the
        /// emitter resolves the PARENT element's var; for the sibling case the node's own
        /// walk var.
        target: ClientNodeId,
        /// The already-assembled second argument (`() => h` thunk, or the bare elided
        /// callee `render`).
        payload: String,
        /// Whether this `{@html}` is the SOLE controlled child of its parent element (the
        /// official `is_controlled` case): operate on the parent var + trailing `true` +
        /// `$.reset(parent)`.
        only_child: bool,
    },
}

/// A narrow supported element LIFECYCLE op — the closed `{Action, Transition,
/// Animation, Attachment}` family behind [`ClientRuntimeOp::Lifecycle`]. Each
/// variant carries its target node id plus the ALREADY-REWRITTEN expression pieces
/// (signal/prop reads lowered through the shared rewriter at plan-build time); the
/// emitter assembles the exact official `svelte/internal/client` call shape with the
/// real DOM var — no emit-time re-inference, no post-hoc string substitution.
///
/// The family keeps each helper distinct BY CONSTRUCTION: an `animate:` is an
/// `Animation` (the `$.animation` helper — never a `$.transition` masquerade), an
/// element `{@attach}` is an `Attachment` (attribute-position — the child form is
/// fail-closed upstream), and the transition FLAG integer is precomputed from the
/// typed kind + `|global` modifier at projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ElementLifecycleOp {
    /// A `use:` action — `$.action(el, ($$node) => callee?.($$node))`, gaining the
    /// `$$action_arg` closure param + the `() => arg` getter thunk when an argument
    /// is present (`use:fn={arg}`).
    Action {
        /// The target node id.
        target: ClientNodeId,
        /// The rewritten action callee (an identifier / member path — `foo`,
        /// `obj.foo`, `$.get(foo)`); the emitter appends the official
        /// optional-chained call (`?.(…)`).
        callee: String,
        /// The rewritten argument THUNK BODY (concise-arrow-wrapped), when the
        /// directive carries `={arg}` — the 3rd positional `() => <arg>` getter.
        arg: Option<String>,
    },
    /// A `transition:` / `in:` / `out:` — `$.transition(FLAG, el, () => fn[, ()
    /// => params])`.
    Transition {
        /// The target node id.
        target: ClientNodeId,
        /// The PRECOMPUTED official flag integer: `TRANSITION_IN(1) |
        /// TRANSITION_OUT(2) | TRANSITION_GLOBAL(4)` — `in`=1, `out`=2,
        /// `transition`=3, `|global` adds 4 (5/6/7); `|local` is the default
        /// (no +4).
        flags: u8,
        /// The rewritten transition-function expression (the directive name resolved
        /// in the op's scope — `fade`, `$.get(fade)`, `$$props.fx`); the emitter
        /// wraps it in the `() => <fn>` getter.
        get_fn: String,
        /// The rewritten params THUNK BODY (concise-arrow-wrapped), present IFF the
        /// directive carries `={params}` — the 4th positional `() => ({ … })`
        /// getter (absent → a 3-argument call).
        params: Option<String>,
    },
    /// An `animate:` — `$.animation(el, () => fn, PARAMS)`, ALWAYS 3 args.
    Animation {
        /// The target node id.
        target: ClientNodeId,
        /// The rewritten animation-function expression (wrapped in `() => <fn>`).
        get_fn: String,
        /// The rewritten params THUNK BODY (concise-arrow-wrapped); `None` emits the
        /// official literal `null` 3rd argument.
        params: Option<String>,
    },
    /// An element-position `{@attach expr}` — `$.attach(el, () => expr)` (2 args).
    Attachment {
        /// The target node id.
        target: ClientNodeId,
        /// The rewritten attachment THUNK BODY (concise-arrow-wrapped — an inline
        /// arrow payload stays a valid expression body: `() => (() => {})`).
        payload: String,
    },
}

impl ElementLifecycleOp {
    /// The target node id (uniform accessor across the four variants).
    pub(super) fn target(&self) -> ClientNodeId {
        match self {
            Self::Action { target, .. }
            | Self::Transition { target, .. }
            | Self::Animation { target, .. }
            | Self::Attachment { target, .. } => *target,
        }
    }

    /// Whether this op emits INLINE during the walk (the init domain — `Action` /
    /// `Attachment`, interleaved with `bind:this` in source order) vs LAST in the
    /// post-walk op stage (`Transition` / `Animation`, after binds + events) — the
    /// official `RegularElement` phase split.
    pub(super) fn is_init_domain(&self) -> bool {
        matches!(self, Self::Action { .. } | Self::Attachment { .. })
    }
}

/// One part of a dynamic attribute value carried to the emitter — a literal text
/// chunk or a rewritten expression with its `has_call` memoize fact. The emitter
/// resolves each [`AttrValuePart::Expr`] through the shared [`super::client::Memoizer`]
/// at emit time, so a `has_call` value lands in the official deps-array form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AttrValuePart {
    /// A literal text chunk of a mixed attribute value (already entity-decoded +
    /// escaped for the backtick template at emit time).
    Literal(String),
    /// A rewritten expression part + whether it `has_call` (drives memoization) + how it
    /// is `?? ''`-coerced (official `build_template_chunk`).
    Expr {
        /// The already-rewritten client expression.
        rewritten: String,
        /// Whether the expression `has_call` (memoized into a `$N` deps-array slot).
        has_call: bool,
        /// How the live part is coerced to a string in the backtick template — the
        /// official `is_defined`/precedence `?? ''` rule (a provably-defined part is
        /// emitted raw, an undecided part gets `?? ''`, parenthesized for a `&&`/`||`
        /// operand).
        coalesce: super::reactive_fold::NullishCoalesce,
    },
}

/// A dynamic attribute / property VALUE carried to the emitter in STRUCTURED form,
/// so the emitter can route each expression through the shared memoizer (the
/// official `has_call` deps-array rule) at emit time — never a pre-flattened string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AttrValue {
    /// A constant value with no expression (`true` for a valueless boolean, a quoted
    /// literal string) — emitted verbatim, never memoized.
    Const(String),
    /// A SINGLE dynamic expression (`id={expr}`) — emitted as the bare (possibly
    /// memoized `$N`) value with NO `?? ''` wrap.
    Single {
        /// The already-rewritten client expression.
        rewritten: String,
        /// Whether the expression `has_call` (memoized into a `$N` deps-array slot).
        has_call: bool,
    },
    /// A MIXED literal+expression value (`id="a{x}b"`) — emitted as the
    /// `` `lit${expr ?? ''}lit` `` template, each expr routed through the memoizer.
    Mixed(Vec<AttrValuePart>),
}

impl AttrValue {
    /// Whether ANY expression part of the value `has_call` — the official
    /// `metadata.expression.has_call` aggregated over the value. A `has_call` value
    /// is MEMOIZED and forces the write into the render `$.template_effect` (the
    /// official `Memoizer.add` rule: `has_call || has_await` memoizes, and a memoized
    /// `$N` placeholder can only be bound by the effect), independent of `has_state`.
    pub(super) fn has_call(&self) -> bool {
        match self {
            AttrValue::Const(_) => false,
            AttrValue::Single { has_call, .. } => *has_call,
            AttrValue::Mixed(parts) => parts
                .iter()
                .any(|p| matches!(p, AttrValuePart::Expr { has_call: true, .. })),
        }
    }
}

/// A DYNAMIC/mixed `value={…}` on a `bind:group` `<input>` — the structured value plus its
/// reactivity. The official `bind:group` value topology (svelte@5.56.3):
///
/// - REACTIVE ⇒ a `var <var>_value;` change-tracker (declared inline in the bind prelude) +
///   a guarded `if (<var>_value !== (<var>_value = V)) { <var>.value = (<var>.__value = V)
///   ?? ''; }` update folded into the combined `$.template_effect` BEFORE `$.bind_group`;
/// - NON-reactive ⇒ a one-shot inline `<var>.value = (<var>.__value = V) ?? ''` write (same
///   position as the static-literal write), no tracker, no effect;
///
/// and in BOTH cases the input's `$.bind_group` getter gains a dependency read of the value
/// (`() => { V; return <target>; }`). A SINGLE-expression value gets the OUTER `?? ''` string
/// coercion; a MIXED template literal (or a folded `Const`) does not (it is already a string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GroupDynamicValue {
    /// The structured value — rendered to the getter dep-read + the non-reactive inline write
    /// WITHOUT the memoizer (full inline), and to the guarded effect write WITH the shared
    /// memoizer (so a `has_call` value becomes a `$N` deps-array slot).
    pub(super) value: AttrValue,
    /// Whether the value is REACTIVE (joins the combined effect, guarded by the tracker) vs
    /// NON-reactive (a one-shot inline direct write). Official `has_state || has_call`.
    pub(super) reactive: bool,
    /// For a SINGLE-expression value ([`AttrValue::Single`]), whether it is PROVABLY DEFINED —
    /// the official `evaluated.is_defined` gate (reused from the `mixed_chunk_nullish_wrap`
    /// definedness analysis). Official gates the outer `?? ''` group-value coercion on
    /// DEFINEDNESS, not single-vs-mixed: a provably-defined single value emits
    /// `var.value = var.__value = V` (NO outer `?? ''`), while an undecided / nullish / reactive
    /// single keeps `var.value = (var.__value = V) ?? ''`. Always `false` for a mixed value
    /// (already a string — it never carries the outer coercion regardless).
    pub(super) single_value_defined: bool,
}

impl GroupDynamicValue {
    /// Whether the value is a SINGLE expression (the official `Dynamic` / `value.length === 1`
    /// shape) — the form that gets the OUTER `?? ''` string coercion. A MIXED template literal
    /// (or a folded `Const`) is already a string, so it gets NO outer coercion.
    pub(super) fn is_single_expression(&self) -> bool {
        matches!(self.value, AttrValue::Single { .. })
    }
}

/// The emission shape of a dynamic plain-attribute write — the official
/// `RegularElement.js` attribute dispatch, decided by `is_dom_property` + the special
/// `autofocus` / `muted` arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientDynAttrEmit {
    /// `$.set_attribute(node, 'name', value)` — the structured value (resolved through
    /// the memoizer at emit time) and the (HTML-lowercased) attribute name.
    SetAttribute {
        /// The serialized attribute name.
        name: String,
        /// The structured value (memoized at emit time).
        value: AttrValue,
    },
    /// `node.<prop> = value` — a DOM-property write (`button.disabled = $.get(v)`,
    /// `input.readOnly = …`, `video.muted = …`).
    Property {
        /// The DOM property name.
        prop: String,
        /// The structured value (memoized at emit time).
        value: AttrValue,
    },
    /// `$.autofocus(node, value)` — the init-only autofocus helper. The value is the
    /// literal `true` (a valueless `autofocus`) or the already-rewritten expression.
    /// Autofocus is ALWAYS init-only (read once), so it is never memoized — a plain
    /// pre-rewritten string suffices.
    Autofocus {
        /// The already-rewritten value expression (or `true`).
        value: String,
    },
}
