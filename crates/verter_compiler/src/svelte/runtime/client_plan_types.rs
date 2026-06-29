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
use super::ir::ExprId;

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
