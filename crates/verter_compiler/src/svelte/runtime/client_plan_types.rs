//! The NARROW client-plan VOCABULARY — the closed type set the client emitter
//! consumes (extracted from `client_plan.rs` to keep it under the file-size guard).
//!
//! Every supported broad-IR node / attribute / op projects to exactly one of these
//! narrow variants in `client_plan.rs`'s `SupportedClientIr::build`; no broad-IR
//! variant reaches emission. These are pure data definitions — the projection logic
//! and the `ClientModulePlan` / `SupportedClientIr` builder stay in `client_plan.rs`.

use verter_span::Span;

use super::client_allowlist::SupportedHtmlElement;
use super::client_legacy_value::PreparedTemplateValue;
use super::client_plan_block_types::{ClientBlock, ClientDebugEntry, ClientDeclaration};
use super::client_shapes::{ClientBindShape, ClientEventHandlerShape};
use super::ir::{EventOrigin, ExprId, LetBinding, TemplateScopeId};
use super::synthesized_value::SynthesizedTemplateValue;

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
    /// A `<slot>` outlet — the projected `$.slot(node, $$props, name, props,
    /// fallback)` call against its walked `<!>` anchor var. Carries ONLY
    /// fully-classified/rewritten data (the semantic name, the planned property
    /// members / spread thunks, the memoizer hoists, the optional fallback
    /// region) — the emitter never recovers these from broad IR or source text.
    Slot(ClientSlot),
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
    /// The PREPARED value expression (via the sole authored-value entry, surface
    /// `BoundaryProp` — policy `Raw`, so the carrier is always the raw rewritten
    /// form): the getter body (`return <expr>;`) or the init value.
    pub(super) value: PreparedTemplateValue,
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
    pub(super) fold: Vec<AttributeEffectItem>,
    /// The scope-hash literal (`'svelte-<hash>'`) the FOLD threads as the official 6th
    /// positional `$.attribute_effect` argument (`build_attribute_effect` —
    /// `element.metadata.scoped && css.hash`), with the intermediate sync/async/blockers
    /// slots as `void 0` — the SAME argument row the regular-element spread fold uses.
    /// `Some` iff the host node is SCOPED; the SetClass fast path carries its hash inside
    /// [`SetClassPieces`] instead (the fold hash is unused when `fold` is empty).
    pub(super) css_hash: Option<String>,
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

/// One source-ordered item of an `$.attribute_effect` fold — the SHARED typed
/// item vocabulary serving BOTH the regular-element spread fold and the
/// `<svelte:element>` fold, so the wrap/memoize rules cannot drift between the
/// two hosts. The emitter renders the items through ONE ordered memoizer (the
/// official per-effect `Memoizer`) producing the arrow params + the single
/// dependency array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AttributeEffectItem {
    /// A pre-rendered SYNTHESIZED/static entry (`name: 'lit'`, the
    /// analyze-phase `class: ''` / `style: ''` synthetics) — emitted verbatim,
    /// never memoized. Carries NO authored expression by construction.
    Entry(String),
    /// A co-located ordinary attribute value (`prop: <value>`) — each authored
    /// expression was PREPARED (`BuildExpression` policy) and memoizes on
    /// `has_call` (the official `build_attribute_value` + `Memoizer.add`).
    Attr {
        /// The fold object KEY (already `object_key`-quoted as needed).
        prop: String,
        /// The structured value (authored parts prepared at planning).
        value: AttrValue,
    },
    /// A spread operand (`...expr`) — RAW with respect to `build_expression`,
    /// but memoized on `has_call` (official `SpreadAttribute` + `Memoizer.add`).
    Spread {
        /// The prepared (raw-policy) operand.
        value: PreparedTemplateValue,
    },
    /// An EVENT-attribute handler with a function-expression value — hoisted to
    /// a stable `var <name> = <handler>;` local (the official attribute-effect
    /// handler-stability hoist), then referenced by name in the fold.
    Event {
        /// The fold object KEY (`onclick`).
        prop: String,
        /// The prepared handler (raw-policy; a function expression never
        /// triggers the wrap).
        handler: PreparedTemplateValue,
    },
    /// The merged `[$.CLASS]: { … }` directive object — SYNTHESIZED (inner
    /// conditions stay raw), memoized as a whole on the merged `has_call`.
    ClassDirectives(SynthesizedTemplateValue),
    /// The merged `[$.STYLE]: { … }` / `[normal, important]` directive
    /// object — SYNTHESIZED (inner values were prepared and wrap), memoized as
    /// a whole on the merged `has_call`.
    StyleDirectives(SynthesizedTemplateValue),
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
    /// The `css_hash` arg: the scope-hash literal when the host is SCOPED and
    /// the hash did not fold into a literal `value` (the official
    /// `build_set_class` 3-way), the `null` placeholder when directives are
    /// present without a hash (`!css_hash && next`), or `None` (omitted).
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

/// A projected `<slot>` outlet — the closed structural mirror of an
/// [`IrNode::Slot`](super::ir::IrNode) node, built (with every prop value
/// rewritten) in `client_slot_plan`. The emitter assembles the official
/// `$.slot(node, $$props, '<name>', <props>, <fallback>)` call from this typed
/// structure; the fallback region is emitted through the shared region emitter
/// at its [`TemplateScopeId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientSlot {
    /// The full open-tag source span.
    pub(super) span: Span,
    /// The DECODED semantic slot name (`'default'` when unnamed) — emitted as a
    /// single-quoted string literal.
    pub(super) name: String,
    /// The ordinary (non-spread) slot props, in source order — the members of
    /// the ONE leading props object (official: `b.object(props)` first, every
    /// spread thunk after; spreads never interleave with the object).
    pub(super) props: Vec<SlotProp>,
    /// The spread thunk texts in source order (`rest` for an unthunked zero-arg
    /// accessor call, `() => o().x` otherwise) — appended AFTER the props object
    /// inside `$.spread_props({ … }, thunk, …)`. Empty ⇒ the plain object form.
    pub(super) spreads: Vec<String>,
    /// The memoized-value hoists (`let $N = $.derived(() => …);`), emitted inside
    /// a wrapping `{ … }` block BEFORE the `$.slot` statement (the official
    /// per-`SlotElement` `Memoizer.deriveds()`). Empty when no value memoizes.
    pub(super) memo_hoists: Vec<String>,
    /// The fallback region, or `None` for the official literal `null` (a slot
    /// with NO raw children — a whitespace-only fallback stays `Some`, emitting
    /// the official empty callback `($$anchor) => {}`).
    pub(super) fallback: Option<TemplateScopeId>,
}

/// One ordinary member of a `$.slot` props object, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SlotProp {
    /// `key: value` — a static literal / boolean / non-reactive value.
    Init {
        /// The prop key.
        key: String,
        /// The already-rewritten value expression.
        value: String,
    },
    /// `get key() { return body; }` — a state-bearing value (the child re-reads
    /// on change).
    Getter {
        /// The prop key.
        key: String,
        /// The already-rewritten getter body.
        body: String,
    },
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
    /// Whether a STATIC callee is the direct OPTIONAL call (`pair?.(node, …)` — a
    /// resolved local-snippet `{@render pair?.(1)}`, the official `b.maybe_call` form)
    /// vs the plain direct call (`pair(node, …)`). Meaningful only when `dynamic` is
    /// `false`.
    pub(super) maybe_call: bool,
    /// The memoized-argument hoist statements (`let $N = $.derived(() => …);`, the
    /// official per-`RenderTag` `Memoizer.deriveds()`), emitted inside a wrapping
    /// `{ … }` block before the render call. Empty when no argument memoizes.
    pub(super) memo_hoists: Vec<String>,
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
        /// The PREPARED interpolation value (the sole authored-value carrier:
        /// rewrite + facts + the surface-policied legacy wrap, computed at the
        /// fallible planning stage). The emitter only serializes it.
        value: PreparedTemplateValue,
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
        /// The `css_hash` arg: the scope-hash literal when the element is
        /// SCOPED and the hash did not fold into a literal `value` (the
        /// official `build_set_class` 3-way), the `null` placeholder when
        /// directives are present without a hash (`!css_hash && next`), or
        /// `None` (omitted).
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
    /// A coalesced `$.attribute_effect(el, (params) => ({ <fold> }), [deps…])` write —
    /// the SINGLE reactive effect a spread element gets in place of the per-attribute
    /// path. The presence of ANY spread on an element switches its WHOLE attribute
    /// strategy: every co-located attribute folds into the single object literal the
    /// effect returns — plain attributes (static / dynamic / mixed / a plain `class` /
    /// `style` attribute) and the spreads themselves IN SOURCE ORDER, with every
    /// `class:` directive merged into ONE trailing `[$.CLASS]` and every `style:`
    /// directive into ONE trailing `[$.STYLE]` appended LAST (the official
    /// `Element.js` spread path) — and the element emits NO separate
    /// `$.set_attribute` / `$.set_class` / `$.set_style` / property write and NO
    /// `$.template_effect`. The op carries the TYPED source-ordered fold items; the
    /// emitter renders them through the shared per-effect memoizer (each `has_call`
    /// value hoists into a `$N` arrow param + a `() => <expr>` dependency — the
    /// official per-attribute/spread `Memoizer` topology) and assembles the
    /// `el, ($0, …) => ({ <body> }), [deps…]` call with the real DOM var.
    AttributeEffect {
        /// The target node id.
        target: ClientNodeId,
        /// The typed source-ordered fold items (plain attrs, spreads, hoisted
        /// event handlers, the merged directive objects).
        items: Vec<AttributeEffectItem>,
        /// Whether the element takes the trailing `true` `remove_defaults`
        /// argument — the official form for an `<input>` (suppressed by an
        /// authored `defaultValue` / `defaultChecked`).
        input_trailing: bool,
        /// The scope-hash literal argument for a SCOPED spread element — the
        /// official `build_attribute_effect` passes `css_hash` at position 6
        /// ("the spread method appends the hash to the end of the class
        /// attribute on its own"). `None` for an unscoped element.
        css_hash: Option<String>,
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
    /// A `$.html(node, <getter> [, true])` raw-markup insertion. The payload was
    /// PREPARED first (the official `build_expression` order); thunk ELISION to the
    /// bare callee applies ONLY when the prepared value is RAW and retains the
    /// eligible direct zero-arg-identifier-call shape whose callee rewrites
    /// unchanged — a legacy-wrapped call is never elided (its getter returns the
    /// prepared sequence). `only_child` selects the topology: a `{@html}` that is
    /// the SOLE controlled child of its parent element operates on the PARENT
    /// element var with the trailing `true` argument and is followed by
    /// `$.reset(parent)`; a `{@html}` with siblings operates on its OWN `<!>`
    /// anchor var (reached by the DOM walk) with NO trailing argument.
    Html {
        /// The target node id (the `{@html}` tag node). For the only-child case the
        /// emitter resolves the PARENT element's var; for the sibling case the node's own
        /// walk var.
        target: ClientNodeId,
        /// The PREPARED payload value.
        payload: PreparedTemplateValue,
        /// The getter topology over the prepared payload (elision decisions are
        /// RAW-carrier-only; a legacy-wrapped payload always keeps the thunk).
        getter_form: HtmlGetterForm,
        /// Whether this `{@html}` is the SOLE controlled child of its parent element (the
        /// official `is_controlled` case): operate on the parent var + trailing `true` +
        /// `$.reset(parent)`.
        only_child: bool,
    },
}

/// The `$.html` GETTER topology over a PREPARED payload — decided at planning,
/// on the RAW carrier only (a legacy-wrapped payload is never elided).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HtmlGetterForm {
    /// The general `() => <prepared>` getter (wrapped payloads always take it).
    PreparedThunk,
    /// The bare elided callee (`render`) — a RAW direct non-optional zero-arg
    /// identifier call whose callee rewrote UNCHANGED (the official `b.thunk`
    /// unthunk).
    ElidedCallee(String),
    /// The `() => <callee>()` thunk REBUILT from the peeled rewritten callee —
    /// a RAW direct zero-arg call whose callee rewrote to a member/getter
    /// (author parens around the callee drop, matching the official printed
    /// AST: `() => $$props.render()`).
    RebuiltCallThunk(String),
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
        /// The PREPARED attachment payload (the emitter supplies the thunk; a
        /// legacy-wrapped payload keeps the thunk over the sequence).
        payload: PreparedTemplateValue,
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
/// chunk or a PREPARED authored expression with its `?? ''` coercion. The emitter
/// resolves each [`AttrValuePart::Expr`] through the shared memoizer at emit time,
/// so a `has_call` value lands in the official deps-array form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AttrValuePart {
    /// A literal text chunk of a mixed attribute value (already entity-decoded +
    /// escaped for the backtick template at emit time).
    Literal(String),
    /// A PREPARED authored expression part + how it is `?? ''`-coerced (official
    /// `build_template_chunk`).
    Expr {
        /// The prepared authored value (rewrite + facts + surface-policied wrap).
        value: PreparedTemplateValue,
        /// How the live part is coerced to a string in the backtick template — the
        /// official `is_defined`/precedence `?? ''` rule (a provably-defined part is
        /// emitted raw, an undecided part gets `?? ''`, parenthesized for a `&&`/`||`
        /// operand). A legacy-wrapped part is always the BARE `?? ''` (a sequence is
        /// never provably defined, and it self-parenthesizes).
        coalesce: super::reactive_fold::NullishCoalesce,
    },
}

/// A SINGLE-expression template value that is either the AUTHORED prepared
/// expression or a SYNTHESIZED composite (the `$.clsx(...)` class-base wrap).
/// The split is BY TYPE: a synthesized value cannot occupy the authored arm
/// (rustc), so the legacy wrap is never applied to (or omitted from) the
/// wrong ARM. The reverse lane — authored raw text entering the synthesized
/// carrier — is sealed against out-of-module struct-literal construction by
/// rustc module privacy (the fields are private to the dedicated owner module);
/// within that module the routing guard re-checks the construction-site +
/// associated-item inventory as a fail-closed structural tripwire over the
/// inventoried construction topology, not a proof that no in-module route can
/// build the carrier. An in-owner permitted method body that derives arbitrary
/// `text` at runtime is outside this structural check; foreclosing it requires a
/// backend-wide typed authored-emission capability boundary rather than a
/// raw-string carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PlannedTemplateValue {
    /// An authored expression, prepared through the sole
    /// [`prepare_template_value`](super::client_plan::SupportedClientIr::prepare_template_value)
    /// entry point.
    Authored(PreparedTemplateValue),
    /// A synthesized composite (never wrap-eligible). The authored inner
    /// expression was prepared BEFORE synthesis (official applies
    /// `build_expression` before `$.clsx`).
    Synthesized(SynthesizedTemplateValue),
}

impl PlannedTemplateValue {
    /// The official `has_call` memoize trigger of the value.
    pub(super) fn has_call(&self) -> bool {
        match self {
            PlannedTemplateValue::Authored(p) => p.has_call(),
            PlannedTemplateValue::Synthesized(s) => s.has_call(),
        }
    }
}

/// One `style:` directive entry of the merged `[$.STYLE]` synthesized object —
/// the raw directive property name (quoted into an object key by the
/// constructor), the TYPED value contributor, and the `|important` flag.
#[derive(Debug)]
pub(super) struct StyleDirectiveObjectEntry {
    /// The raw `style:<property>` name (quoted by the constructor).
    pub(super) property: String,
    /// The typed value contributor.
    pub(super) value: StyleDirectiveObjectValue,
    /// Whether the directive carries `|important` (the array-form switch).
    pub(super) important: bool,
}

/// The TYPED value contributor of one style-directive object entry — an
/// authored PREPARED expression, a static text (quoted by the constructor), or
/// a mixed template whose authored chunks were prepared upstream (folded by
/// the constructor through the shared template fold). No free-text arm exists:
/// every authored contribution enters as a prepared/typed carrier.
#[derive(Debug)]
pub(super) enum StyleDirectiveObjectValue {
    /// An authored prepared expression value (`style:width={w}`).
    Prepared(PreparedTemplateValue),
    /// A static text value (`style:color="red"`) — single-quoted HERE.
    StaticText(String),
    /// A mixed text+interpolation value (`style:color="a{x}b"`) whose authored
    /// chunks were prepared upstream — folded HERE.
    Mixed(AttrValue),
}

/// A dynamic attribute / property VALUE carried to the emitter in STRUCTURED form,
/// so the emitter can route each expression through the shared memoizer (the
/// official `has_call` deps-array rule) at emit time — never a pre-flattened string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AttrValue {
    /// A constant value with no expression (`true` for a valueless boolean, a quoted
    /// literal string) — emitted verbatim, never memoized.
    Const(String),
    /// A SINGLE value (`id={expr}`, the class/style base) — authored-prepared or a
    /// synthesized composite, emitted as the bare (possibly memoized `$N`) value
    /// with NO `?? ''` wrap.
    Single(PlannedTemplateValue),
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
            AttrValue::Single(v) => v.has_call(),
            AttrValue::Mixed(parts) => parts
                .iter()
                .any(|p| matches!(p, AttrValuePart::Expr { value, .. } if value.has_call())),
        }
    }

    /// Shorthand: a single AUTHORED prepared value.
    pub(super) fn single_authored(prepared: PreparedTemplateValue) -> Self {
        AttrValue::Single(PlannedTemplateValue::Authored(prepared))
    }

    /// Fold the structured value to its FULL inline text — the memoizer-free
    /// rendering used where the whole value embeds as one expression (a
    /// `style:` directive-object member, the non-reactive `bind:group` value
    /// write). A mixed value folds to the `` `lit${expr ?? ''}lit` `` template
    /// with each prepared chunk inlined (wrap-parenthesized where wrapped) and
    /// its `?? ''` coercion applied per the recorded [`AttrValuePart`] fact.
    pub(super) fn folded_text(&self) -> String {
        match self {
            AttrValue::Const(text) => text.clone(),
            AttrValue::Single(PlannedTemplateValue::Authored(p)) => p.inline_expression(),
            AttrValue::Single(PlannedTemplateValue::Synthesized(s)) => s.raw_text().to_string(),
            AttrValue::Mixed(parts) => {
                let mut tmpl = String::from("`");
                for part in parts {
                    match part {
                        AttrValuePart::Literal(text) => {
                            tmpl.push_str(&super::client_codegen_helpers::escape_template_text(
                                text,
                            ));
                        }
                        AttrValuePart::Expr { value, coalesce } => {
                            use super::reactive_fold::NullishCoalesce;
                            let v = value.inline_expression();
                            match coalesce {
                                NullishCoalesce::None => tmpl.push_str(&format!("${{{v}}}")),
                                NullishCoalesce::Bare => {
                                    tmpl.push_str(&format!("${{{v} ?? ''}}"));
                                }
                                NullishCoalesce::Parenthesized => {
                                    tmpl.push_str(&format!("${{({v}) ?? ''}}"));
                                }
                            }
                        }
                    }
                }
                tmpl.push('`');
                tmpl
            }
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
