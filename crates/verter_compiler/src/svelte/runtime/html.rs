//! Static-HTML synthesis + DOM-path / node-path planning.
//!
//! [`plan_static_templates`] walks the runtime IR's root template scope and
//! synthesises the static-HTML skeleton each top-level template region needs,
//! plus the dynamic slots (reactive text / attributes / interpolations) and the
//! client-side node-path walk (`first_child` / `child` / `sibling` / `reset` /
//! `next`) that reaches each dynamic node.
//!
//! The HTML skeleton + the dynamic-slot list are SHARED with the server backend:
//! the server consumes [`StaticTemplatePlan::templates`] + the dynamic slots and
//! ignores [`StaticTemplatePlan::client_paths`] (the client-only DOM walk). The
//! fragment-flag rule is hard-coded from the conformance oracle: `Some(FragmentOne)`
//! for ANY multi-root template (2 roots or 10), ABSENT for exactly one root; a
//! zero-element / block-only root becomes a comment-anchor factory, not a
//! `from_html`.

use super::entity_decode::{decode_text_entities, escape_html_attr};
use super::ir::{
    AttrIr, EscapeMode, ExprId, IrNode, NodeId, SpecialKind, StaticAttrValue, StyleDirectiveValue,
    SvelteRuntimeIr, TemplateScope, TemplateScopeId,
};
use super::whitespace::{clean_nodes, is_html_ws, CleanContext, CleanItem};

/// The static-template plan for a component: the template factories, the dynamic
/// slots, and the client-side node-path plans.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StaticTemplatePlan {
    /// The static-template factories in template-region order. A component with a
    /// single root template region produces one factory; nested block bodies that
    /// are themselves template regions each contribute their own factory.
    pub templates: Vec<TemplateFactory>,
    /// The dynamic slots (reactive text / attributes / interpolations) the
    /// template region needs, in walk order. Shared with the server backend.
    pub slots: Vec<DynamicSlot>,
    /// The client-side node-path plans (the DOM walk reaching each dynamic node).
    /// Client-ONLY — the server backend ignores this.
    pub client_paths: Vec<NodePathPlan>,
}

/// A static-template factory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateFactory {
    /// A `$.from_html("…", flag)` factory — the serialized static HTML skeleton
    /// plus the optional fragment flag.
    FromHtml {
        /// The serialized static HTML (interpolation slots collapsed to a single
        /// space, dynamic attributes elided).
        html: String,
        /// The fragment flag, `Some(FragmentOne)` for a multi-root template.
        fragment_flag: Option<TemplateFlag>,
    },
    /// A `$.text(seed)` text-node factory — the official "text-first" region
    /// optimization (`svelte@5.56.3`): when a region's whole cleaned sequence
    /// reduces to a SINGLE text run (pure static text, or one-or-more
    /// interpolations with no element/block sibling), the runtime creates the
    /// region's root as a bare text node via `$.text(...)` rather than cloning a
    /// `from_html` template. This is a RUNTIME distinction — a `from_html` clone
    /// vs a text node — not a cosmetic one.
    TextNode {
        /// The seed text the node is created with: `Some("hello")` for a PURE
        /// static-text run (official `$.text('hello')`), `None` for a run that
        /// contains any interpolation (official `$.text()` — the empty text node
        /// the reactive `$.set_text` fills).
        seed: Option<String>,
    },
    /// A `$.comment()` anchor factory — a zero-element / block-only root that has
    /// no static HTML skeleton.
    CommentAnchor {
        /// Why the region is a comment anchor.
        reason: AnchorReason,
    },
    /// A STANDALONE component / `{@render}` root — the official `is_standalone`
    /// optimization (`phases/3-transform/utils.js`): a region whose SOLE cleaned
    /// node is a non-dynamic `<Component>` (with no `--css-var` attribute) or a
    /// non-dynamic `{@render}` tag emits NO static template. The runtime calls the
    /// component / renders the snippet against the PARENT block's anchor directly
    /// (a top-level `<Foo/>` → `Foo($$anchor, {})`, no `from_html`, no `$.append`
    /// of a cloned fragment). EMPIRICALLY confirmed against svelte@5.56.3. This is
    /// a RUNTIME-shape distinction — no template clone vs a `<!>` `from_html`.
    Standalone {
        /// Whether the standalone root is a component or a `{@render}` tag.
        kind: StandaloneKind,
    },
}

/// The kind of [`TemplateFactory::Standalone`] root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandaloneKind {
    /// A `<Component>` root (`Foo($$anchor, {})`).
    Component,
    /// A `{@render}` tag root (`$.snippet(anchor, …)`).
    Render,
}

/// The static-template trailing flag — the official `$.from_html(html, flags)`
/// bitmask (`svelte@5.56.3` `src/constants.js`). A combination of:
/// `TEMPLATE_FRAGMENT = 1` (a MULTI-ROOT fragment) and `TEMPLATE_USE_IMPORT_NODE =
/// 1 << 1 = 2` (the template needs `importNode` — set when it contains a
/// `<video>` or a CUSTOM element). The `SVG`/`MathML` bits (`4`/`8`) are owned by
/// the deferred namespace-aware root-helper layer and are not produced here.
/// A flag of value 0 is represented as `None` (no trailing argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateFlag(u8);

impl TemplateFlag {
    /// `TEMPLATE_FRAGMENT` (multi-root) — the trailing `1` bit.
    pub const FRAGMENT: u8 = 1;
    /// `TEMPLATE_USE_IMPORT_NODE` — the `1 << 1 = 2` bit (a `<video>` / custom
    /// element in the template, so the runtime must clone via `importNode`).
    pub const USE_IMPORT_NODE: u8 = 1 << 1;

    /// Build a flag from the raw bits, returning `None` for an empty (0) flag (no
    /// trailing argument is emitted).
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits == 0 {
            None
        } else {
            Some(Self(bits))
        }
    }

    /// The raw bitmask value.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether the multi-root fragment bit is set.
    #[must_use]
    pub const fn is_fragment(self) -> bool {
        self.0 & Self::FRAGMENT != 0
    }

    /// Whether the `importNode` bit is set.
    #[must_use]
    pub const fn uses_import_node(self) -> bool {
        self.0 & Self::USE_IMPORT_NODE != 0
    }

    /// The literal trailing-flag token the backend emits (the decimal bitmask).
    #[must_use]
    pub fn literal(self) -> String {
        self.0.to_string()
    }
}

/// Why a template region is a comment anchor rather than a static-HTML factory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorReason {
    /// The region has no element/text roots — only block constructs and/or tags.
    BlockOnlyRoot,
    /// The region is empty.
    EmptyRoot,
    /// The region's SOLE root is a `{@html}` raw-markup tag (a `$.comment()`-anchored
    /// root: `var fragment = $.comment(); var node = $.first_child(fragment); $.html(node,
    /// () => h);`). Distinct from a block-only root so the client backend emits the
    /// supported raw-markup root frame instead of failing closed.
    RawHtmlRoot,
}

/// A dynamic slot the template region needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicSlot {
    /// The template scope (region) this slot belongs to — the root scope, an
    /// `{#each}` body, an `{#if}` branch, an `{#await}` branch, a `{#snippet}`
    /// body, … . Slots are REGION-INDEXED so a backend resolves a nested-block
    /// body's dynamic surface to its owning region.
    pub scope: TemplateScopeId,
    /// The node the slot targets.
    pub node: NodeId,
    /// The slot kind.
    pub kind: DynamicSlotKind,
}

/// The kind of dynamic slot.
///
/// This is the SSR-reusable dynamic-surface list: every reactive surface the
/// server backend must render dynamically — reactive text, raw `{@html}`,
/// attributes, class / style directives, spreads, two-way binds, and block
/// anchors — has a slot kind so the server's renderer never re-derives a surface
/// the IR already knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicSlotKind {
    /// A reactive text interpolation (`{expr}`).
    Text {
        /// The interpolated expression.
        expr: ExprId,
        /// The escape mode.
        escape: EscapeMode,
    },
    /// A raw-markup `{@html expr}` slot (inserted without escaping).
    Html {
        /// The raw-markup expression.
        expr: ExprId,
    },
    /// A dynamic attribute (`id={expr}`).
    Attribute {
        /// The attribute name.
        name: String,
        /// The value expression.
        expr: ExprId,
    },
    /// A `class:name={cond}` directive.
    Class {
        /// The class name.
        name: String,
        /// The condition expression, or `None` for the shorthand `class:name`.
        condition: Option<ExprId>,
    },
    /// A `style:prop={value}` directive.
    Style {
        /// The style property name.
        property: String,
        /// The value expression, or `None` for the shorthand `style:prop`.
        value: Option<ExprId>,
    },
    /// A spread-attributes slot (`{...rest}`).
    Spread {
        /// The spread expression.
        expr: ExprId,
    },
    /// A two-way `bind:` slot.
    Bind {
        /// The bind target (`value`, `checked`, …).
        target: String,
        /// The bound expression, or `None` for the shorthand `bind:value`.
        expr: Option<ExprId>,
    },
    /// A block anchor (an `{#if}` / `{#each}` / … block occupies this slot).
    Block,
}

/// The base reference a client-side node path walks from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathBase {
    /// The cloned template fragment root.
    Fragment,
    /// A previously-named node (e.g. a parent reached via an earlier path).
    Node(NodeId),
}

/// One step of a client-side node-path walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodePathStep {
    /// `$.first_child(node)` — descend to a fragment's first child.
    FirstChild,
    /// `$.child(node[, transparent])` — descend to an element's first child.
    Child {
        /// Whether the child is reached transparently (skipping an anchor).
        transparent: bool,
    },
    /// `$.sibling(node, offset)` — advance to a following sibling.
    Sibling {
        /// The sibling offset.
        offset: u32,
    },
    /// `$.reset(node)` — reset the cursor after an element's children.
    Reset,
    /// `$.next()` — advance the cursor to the next anchor.
    Next,
    /// `$.text([init])` — materialise a text node, optionally seeded.
    Text {
        /// The initial text expression, if seeded.
        init: Option<ExprId>,
    },
}

/// The client-side node-path plan reaching a dynamic node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePathPlan {
    /// The template scope (region) this path is built for. The path's
    /// [`PathBase::Fragment`] refers to THIS region's own cloned fragment, so a
    /// nested block-body region's walk is self-contained from its own fragment —
    /// paths are REGION-INDEXED.
    pub scope: TemplateScopeId,
    /// The node this path reaches.
    pub node: NodeId,
    /// The base the path walks from.
    pub base: PathBase,
    /// The walk steps in order.
    pub steps: Vec<NodePathStep>,
}

/// A NON-BODY special element (`<svelte:head>`, `<svelte:options>`,
/// `<svelte:window>`, `<svelte:document>`, `<svelte:body>`) renders NO body
/// content: it owns its own region / window-listener / compile-option fold and is
/// EXCLUDED from the body static-HTML skeleton entirely — it is neither a root nor
/// a `<!>` anchor (verified against `svelte@5.56.3`: a `<svelte:head>…</svelte:head>`
/// before a `<div>` produces a body skeleton of just `<div>`, not `<!> <div>`).
///
/// `<svelte:element>` / `<svelte:component>` / `<svelte:self>` / `<svelte:boundary>`
/// / `<svelte:fragment>` ARE renderable body content (they keep their `<!>` anchor)
/// and are NOT excluded here.
///
/// TODO(follow-up): the full lowering of these non-body specials is owned by
/// downstream layers and is intentionally out of scope here (this only excludes
/// them from the body skeleton). The `<svelte:head>` / window / document / body
/// region lowering (the `$.head(...)` region, the window/document/body listener
/// wiring) is the special-element region-lowering layer; the
/// `<svelte:options namespace>` root-helper selection (`from_svg` / `from_mathml`)
/// plus the options fold is the namespace-aware root-helper selection layer. See
/// the topology-oracle deferral ledger
/// (`runtime_tests.rs::topology_oracle::DEFERRAL_LEDGER`).
pub(super) fn is_non_body_special(node: &IrNode) -> bool {
    matches!(
        node,
        IrNode::Special(s) if matches!(
            s.kind,
            SpecialKind::Head
                | SpecialKind::Options
                | SpecialKind::Window
                | SpecialKind::Document
                | SpecialKind::Body
        )
    )
}

/// A NON-RENDERING template construct emits NO body content: a `{@const}` /
/// `{const}` / `{let}` declaration, a `{@debug}` / `{@attach}`, or a `{#snippet}`
/// DECLARATION (a callable definition, not body output). It is EXCLUDED from the
/// body static-HTML skeleton entirely — never a root, never a `<!>` anchor, never a
/// body-position shift (verified against `svelte@5.56.3`: an `{@const}` before a
/// `<li>` produces just `<li>`, a `{#snippet}` declaration before a `<div>`
/// produces just `<div>`).
///
/// A `{@render}` / `{@html}` tag, and a `{#if}` / `{#each}` / `{#await}` / `{#key}`
/// block, DO render body content (they keep their `<!>` anchor) and are NOT
/// excluded here.
pub(super) fn is_non_rendering_node(node: &IrNode) -> bool {
    use crate::svelte::runtime::ir::{BlockIr, TagIr};
    matches!(
        node,
        IrNode::Tag(
            TagIr::LegacyConst { .. }
                | TagIr::Declaration { .. }
                | TagIr::Debug { .. }
                | TagIr::Attach { .. }
        ) | IrNode::Block(BlockIr::Snippet { .. })
    )
}

/// Whether a node is a "real root" that contributes to the static-HTML skeleton
/// (an element / component / renderable special element / NON-whitespace text /
/// comment), vs a block / interpolation / whitespace-only text / non-body special /
/// non-rendering construct that becomes a comment anchor, a dynamic slot, or is
/// excluded entirely. A whitespace-only text run (the formatting between a script
/// block and a block construct) does NOT make a root static, and a NON-BODY special
/// (`<svelte:head>` / `<svelte:options>` / window / document / body) or a
/// NON-RENDERING construct (`{@const}` / `{#snippet}` decl / …) is never a static
/// root.
fn is_static_html_root(node: &IrNode) -> bool {
    match node {
        IrNode::Special(_) => !is_non_body_special(node),
        IrNode::Element(_) | IrNode::Component(_) | IrNode::Comment { .. } => true,
        // HTML-significance uses the ASCII `is_html_ws` set, NOT `str::trim`
        // (which also folds a literal NBSP `\u{00a0}` and other Unicode
        // whitespace the browser/Svelte treat as significant content).
        IrNode::Text { text, .. } => !text.chars().all(is_html_ws),
        IrNode::Interpolation { .. } | IrNode::Block(_) | IrNode::Tag(_) => false,
    }
}

// The cleaning / run-partition core (the namespace-aware `clean_nodes`, the
// `CleanItem` partition, and the whitespace + `<pre>`-newline rules) lives in the
// sibling [`super::whitespace`] module — the single whitespace + run-partition
// authority the skeleton serializer and the node-path walk both key on.

/// Serialize a CLEANED node sequence into the static-HTML skeleton, with NO
/// inter-node separator (the official skeleton concatenates directly). A
/// `TextRun` emits its skeleton text (raw cleaned text, or a single ` `
/// placeholder for a dynamic run); a `Node` is serialized in place (an element
/// recurses; any other rendered node — component / block / `{@html}` / renderable
/// special / comment — is a `<!>` hydration anchor). `ctx` is the cleaning context
/// the items were produced under (the namespace / parent / whitespace state).
fn serialize_clean_items(
    ir: &SvelteRuntimeIr,
    items: &[CleanItem],
    ctx: CleanContext,
    html: &mut String,
) {
    for item in items {
        match item {
            CleanItem::TextRun { text, .. } => html.push_str(text),
            CleanItem::Node(id) => match ir.node(*id) {
                IrNode::Element(el) => serialize_element(ir, el, ctx, html),
                // A component / renderable special / block / tag / comment at a
                // rendered position is a `<!>` anchor the DOM walk resolves.
                _ => html.push_str("<!>"),
            },
        }
    }
}

/// Serialize an element and its children into the skeleton. `ctx` is the cleaning
/// context the element sits in; its children are cleaned/serialized under the
/// CHILD context ([`CleanContext::for_children_of`]) — the namespace propagates,
/// the whitespace-preservation flag stays set once a `<pre>` / `<textarea>`
/// ancestor turned it on, and the SVG-`<text>` flag tracks for `can_remove_entirely`.
fn serialize_element(
    ir: &SvelteRuntimeIr,
    el: &crate::svelte::runtime::ir::ElementIr,
    ctx: CleanContext,
    html: &mut String,
) {
    html.push('<');
    html.push_str(&el.tag);
    // A CUSTOM element sets ALL its attributes via PROPERTIES at runtime, so its
    // static attributes are NOT serialized into the skeleton — EXCEPT the `is`
    // attribute, which must stay in the cloned HTML for the customized-built-in
    // upgrade (official: `is_static_element` is false for a custom element, and the
    // `is` attribute is the one exception kept in the template). A plain element
    // (including `<video>`, which needs `importNode` but is NOT a custom element)
    // serializes its static attributes normally.
    serialize_static_attrs(&el.attrs, is_custom_element(el), html);
    if el.children.is_empty() && is_void_element(&el.tag) {
        html.push_str("/>");
        return;
    }
    html.push('>');
    serialize_element_children(ir, &el.children, ctx.for_children_of(&el.tag), html);
    html.push_str("</");
    html.push_str(&el.tag);
    html.push('>');
}

/// Serialize an element's children into the skeleton, via the shared
/// [`clean_nodes`] partition (the same one a template region's roots use). `ctx`
/// is the CHILD cleaning context (namespace / parent tag / whitespace state).
///
/// The official `is_controlled` optimization (`process_children`): an `{#each}`
/// block or an `{@html}` tag that is the SOLE cleaned child of an element is
/// CONTROLLED — the runtime walks it without a `<!>` hydration anchor, so the
/// element body stays empty in the skeleton. Only `EachBlock` / `HtmlTag` qualify.
fn serialize_element_children(
    ir: &SvelteRuntimeIr,
    children: &[NodeId],
    ctx: CleanContext,
    html: &mut String,
) {
    let items = clean_nodes(ir, children, ctx);
    if is_sole_controlled(ir, &items) {
        return;
    }
    serialize_clean_items(ir, &items, ctx, html);
}

/// Whether a cleaned child sequence is EXACTLY one controlled `{#each}` / `{@html}`
/// node (the official `is_controlled` case): the runtime mounts it through its own
/// anchor with no `<!>` marker, so the element body is empty in the skeleton.
fn is_sole_controlled(ir: &SvelteRuntimeIr, items: &[CleanItem]) -> bool {
    let [CleanItem::Node(only)] = items else {
        return false;
    };
    matches!(
        ir.node(*only),
        IrNode::Block(crate::svelte::runtime::ir::BlockIr::Each { .. })
            | IrNode::Tag(crate::svelte::runtime::ir::TagIr::Html { .. })
    )
}

/// Append the static attributes of an element to the skeleton (`class="x"`),
/// eliding dynamic / directive attributes (resolved by the DOM walk + ops).
///
/// This handles the GENERAL static-attribute case (escaping, quoting). It does
/// NOT special-case bind-driven input-default removal: the official compiler
/// strips a static `value` / `checked` / `group` default from the template when a
/// `bind:value` / `bind:group` is present on the same input (emitting
/// `$.remove_input_defaults`).
///
/// TODO(follow-up): bind-aware input-default removal (`$.remove_input_defaults`
/// plus the static-template default stripping for `bind:group` / `bind:value`) is
/// part of the dedicated bindings-breadth work and is intentionally out of scope
/// for this serializer — the static default is preserved here, and the
/// bind-aware stripping is owned by the bindings layer.
fn serialize_static_attrs(attrs: &[AttrIr], is_custom: bool, html: &mut String) {
    // A SPREAD on the element switches its WHOLE attribute strategy to the single
    // `$.attribute_effect` fold: EVERY co-located attribute — including the static ones —
    // moves into the runtime object literal, so NONE is baked into the cloned skeleton
    // (the official `Element.js` spread path emits a bare `<div></div>`).
    if attrs.iter().any(|a| matches!(a, AttrIr::Spread { .. })) {
        return;
    }
    // The official `RegularElement.js` rule: a static `class` / `style` stays baked
    // into the skeleton ONLY when the element carries NO `class:` / `style:`
    // directive — a directive pulls the attribute OUT into the merged `$.set_class`
    // / `$.set_style` (the base value becomes the call's `value` arg). Scan once.
    let has_class_directive = attrs.iter().any(|a| matches!(a, AttrIr::Class { .. }));
    let has_style_directive = attrs.iter().any(|a| matches!(a, AttrIr::Style { .. }));
    // The official `bind:group` form emits a static `value="X"` as a runtime
    // `input.value = input.__value = 'X'` write (the group-value `__value` source),
    // NOT a baked static `value` attr — so a `value` on a `bind:group` input is pulled
    // OUT of the cloned skeleton (the pinned svelte@5.56.3 group template is a bare
    // `<input type="radio"/>`).
    let has_group_bind = attrs
        .iter()
        .any(|a| matches!(a, AttrIr::Bind { target, .. } if target == "group"));
    for attr in attrs {
        if let AttrIr::Static { name, value } = attr {
            // A static `value` on a `bind:group` input is pulled out of the skeleton
            // (it becomes the runtime `__value` write).
            if name == "value" && has_group_bind {
                continue;
            }
            // A "cannot be set statically" attribute (`autofocus` / `muted` /
            // `defaultValue` / `defaultChecked`) is NEVER in the skeleton — it is
            // applied at runtime via a property write / `$.autofocus` (the
            // `NonStaticProperty` op the ops pass emits). The official
            // `cannot_be_set_statically` exclusion.
            if cannot_be_set_statically(name) {
                continue;
            }
            // A static `class` / `style` whose element ALSO carries a `class:` /
            // `style:` directive is pulled OUT of the skeleton (its value becomes the
            // base arg to the merged `$.set_class` / `$.set_style`).
            if (name == "class" && has_class_directive) || (name == "style" && has_style_directive)
            {
                continue;
            }
            // A CUSTOM element's attributes are set via PROPERTIES at runtime, so
            // they are NOT in the skeleton — EXCEPT `is`, which stays for the
            // customized-built-in upgrade.
            if is_custom && name != "is" {
                continue;
            }
            // The official compiler DROPS a static `class` whose value is the EXACTLY
            // EMPTY string (`<div class="">` → `<div>`) — an empty class has no
            // effect. A valueless `class` (`<div class>`, `value: None`) is NOT this
            // case (it serializes as `class=""`), and `class=" "` (a space) is kept.
            if name == "class"
                && matches!(value, Some(StaticAttrValue { value }) if value.is_empty())
            {
                continue;
            }
            html.push(' ');
            // The official client template serializer lowercases a static attribute
            // NAME on an HTML element (`template.js`: `is_html ? key.toLowerCase() :
            // key`). Every element the supported client surface serializes is HTML
            // (an SVG / MathML element fails closed at the element allowlist gate), so
            // the name is unconditionally ASCII-lowercased — `data-FooBar` →
            // `data-foobar`, `aria-LabelledBy` → `aria-labelledby`. The allowlisted
            // names (`id` / `class` / `href` / `type` / …) are already lowercase, so
            // only the case-preserving `data-*` / `aria-*` families observe a change.
            html.push_str(&name.to_ascii_lowercase());
            // The official skeleton emits an EMPTY double-quoted value for a
            // valueless boolean attribute (`<input disabled>` → `disabled=""`) — the
            // attribute is present in the cloned HTML as `name=""`, NOT bare `name`.
            // A valued attribute serializes its decoded + re-escaped value.
            match value {
                Some(StaticAttrValue { value }) => {
                    html.push_str("=\"");
                    html.push_str(&escape_html_attr(value));
                    html.push('"');
                }
                None => html.push_str("=\"\""),
            }
        }
    }
}

/// Whether a node is a STANDALONE root (the official `is_standalone`): it emits NO
/// static template and is mounted against the parent block's anchor directly.
/// Returns the [`StandaloneKind`] for a standalone node, or `None` otherwise.
///
/// A node is standalone iff it is:
/// - a `<Component>` that is not dynamic (Verter's `IrNode::Component` is a static
///   component reference; `<svelte:component>` is a dynamic `Special` and is NOT
///   standalone) and bears NO `--css-var` attribute (the official
///   `!first.attributes.some(attr => attr.name.startsWith('--'))`; HMR is off in
///   this pipeline so the `!hmr` clause always holds); or
/// - a non-dynamic `{@render}` tag — a RESOLVED local-snippet call
///   ([`RenderCallee::Snippet`]); a dynamic-callee render
///   ([`RenderCallee::Dynamic`]) is NOT standalone (the official
///   `!first.metadata.dynamic`, where `metadata.dynamic = binding.kind !==
///   'normal'`).
fn standalone_kind(node: &IrNode) -> Option<StandaloneKind> {
    use crate::svelte::runtime::ir::{RenderCallee, TagIr};
    match node {
        IrNode::Component(c) => {
            // A `--css-var` attribute (`<Foo --x="red">`) forces a
            // `svelte-css-wrapper` `from_html` template — NOT standalone.
            let has_css_var = c.attrs.iter().any(|a| match a {
                AttrIr::Static { name, .. }
                | AttrIr::Dynamic { name, .. }
                | AttrIr::Mixed { name, .. } => name.starts_with("--"),
                _ => false,
            });
            (!has_css_var).then_some(StandaloneKind::Component)
        }
        // A resolved local-snippet `{@render row()}` is non-dynamic → standalone.
        // A dynamic-callee render (`{@render getSnippet()?.()}` / a prop snippet)
        // is dynamic → NOT standalone (it stays a block-only comment anchor).
        IrNode::Tag(TagIr::Render {
            callee: RenderCallee::Snippet(_),
            ..
        }) => Some(StandaloneKind::Render),
        _ => None,
    }
}

/// Whether an attribute "cannot be set statically" — the official
/// `cannot_be_set_statically` (`src/utils.js`): the closed set `autofocus` /
/// `muted` / `defaultValue` / `defaultChecked` (`NON_STATIC_PROPERTIES`). Such an
/// attribute is EXCLUDED from the static `from_html` skeleton and applied at
/// runtime (a property write, or `$.autofocus`). Case-sensitive, matching the
/// official exact-name membership.
pub(super) fn cannot_be_set_statically(name: &str) -> bool {
    matches!(
        name,
        "autofocus" | "muted" | "defaultValue" | "defaultChecked"
    )
}

/// Whether an attribute is a DYNAMIC surface the DOM walk must reach (so its host
/// element needs a named walk var) — anything other than a plain baked static
/// attribute. A reactive `Dynamic` / `Mixed`, a `class:` / `style:` / `bind:` /
/// `use:` / transition / event directive, a spread, OR a `cannot_be_set_statically`
/// STATIC attribute (`autofocus` / `muted` / `defaultValue` / `defaultChecked`, which
/// is applied at runtime via `$.autofocus` / a property write, NOT baked) all count.
/// A plain baked `Static` attribute does NOT.
pub(super) fn attr_is_dynamic_surface(attr: &AttrIr) -> bool {
    match attr {
        AttrIr::Static { name, .. } => cannot_be_set_statically(name),
        _ => true,
    }
}

/// The HTML void-element set (self-closing in the static skeleton).
fn is_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Whether a node is a whitespace-only text run (insignificant formatting that
/// the official compiler drops from the skeleton).
///
/// HTML-significance is decided over the ASCII [`is_html_ws`] set (` \t\r\n`) — the
/// official `clean_nodes` whitespace set — NOT Rust `trim()` / `char::is_whitespace`,
/// which would also fold a literal NBSP (`\u{00a0}`) and other Unicode whitespace
/// that the browser (and Svelte) treat as SIGNIFICANT content.
fn is_whitespace_text(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    matches!(ir.node(node_id), IrNode::Text { text, .. } if text.chars().all(is_html_ws))
}

/// Synthesise the static-HTML skeleton + the fragment-flag decision for one
/// template region's root nodes, via the SHARED [`clean_nodes`] partition (the
/// same one element children use — the official compiler runs the same
/// `clean_nodes` at every level). There is NO inter-root separator: the cleaned
/// sequence's `TextRun`s are the ONLY source of inter-node whitespace (adjacent
/// element roots concatenate directly, matching `svelte@5.56.3`).
pub(super) fn synthesize_region(ir: &SvelteRuntimeIr, scope: &TemplateScope) -> TemplateFactory {
    // A region's roots are at the fragment level (HTML namespace, no parent
    // element, never inside a `<pre>`), so cleaning starts from the region-root
    // context.
    let ctx = CleanContext::region_root();
    let items = clean_nodes(ir, &scope.roots, ctx);
    if items.is_empty() {
        return TemplateFactory::CommentAnchor {
            reason: AnchorReason::EmptyRoot,
        };
    }
    // Text-first region (`svelte@5.56.3`): a region whose WHOLE cleaned sequence is
    // a SINGLE text run (pure static text, or interpolation-only, with no
    // element/block sibling) is created as a bare text node via `$.text(seed)`,
    // NOT a `from_html` clone. The seed is the static text for a pure-text run
    // (`$.text('hello')`), or empty for a run with any interpolation (`$.text()` —
    // the reactive `$.set_text` fills it). This is a RUNTIME distinction.
    if let [CleanItem::TextRun { text, interps }] = items.as_slice() {
        let seed = if interps.is_empty() {
            // The `$.text(seed)` seed is a JS STRING, so the text-context entity
            // references are DECODED (`&copy;` → `©`, `&#65;` → `A`, the
            // `&#9999999999;` overflow → NUL) — the official
            // `decode_character_references(text, /*is_attribute_value*/ false)`. This
            // is distinct from the static `from_html` skeleton, whose cloned-HTML
            // template keeps the RAW entities (the browser decodes on clone).
            Some(decode_text_entities(text))
        } else {
            None
        };
        return TemplateFactory::TextNode { seed };
    }
    // STANDALONE root (`is_standalone`): a region whose SOLE cleaned node is a
    // non-dynamic `<Component>` (no `--css-var` attribute) or a `{@render}` tag
    // emits NO static template — the runtime calls the component / renders the
    // snippet against the parent block's anchor (or `$$anchor` at the top level).
    // This is checked BEFORE the block-only-root / from_html paths: a component is
    // otherwise a `<!>`-anchored `from_html` root, which is the X8 bug.
    if let [CleanItem::Node(only)] = items.as_slice() {
        if let Some(kind) = standalone_kind(ir.node(*only)) {
            return TemplateFactory::Standalone { kind };
        }
    }
    // A SINGLE block-only root (a lone `{#if}`/`{#each}`/… anchor that serializes to
    // `<!>`) is a `$.comment()` anchor. MULTIPLE block-only roots are a `from_html`
    // of comment markers (`<!><!>`) with the fragment flag.
    if let [CleanItem::Node(only)] = items.as_slice() {
        if !is_static_html_root(ir.node(*only)) {
            // A lone `{@html}` root is a SUPPORTED `$.comment()`-anchored raw-markup root
            // (a block-only root is also a supported `$.comment()`-anchored root, via a
            // different `AnchorReason`).
            let reason = if matches!(
                ir.node(*only),
                IrNode::Tag(crate::svelte::runtime::ir::TagIr::Html { .. })
            ) {
                AnchorReason::RawHtmlRoot
            } else {
                AnchorReason::BlockOnlyRoot
            };
            return TemplateFactory::CommentAnchor { reason };
        }
    }
    let mut html = String::new();
    serialize_clean_items(ir, &items, ctx, &mut html);
    // The trailing flag is the official `from_html` bitmask: TEMPLATE_FRAGMENT (1)
    // for a MULTI-ROOT template (2+ cleaned DOM positions), OR'd with
    // TEMPLATE_USE_IMPORT_NODE (2) when the template contains a `<video>` or a
    // CUSTOM element (so the runtime clones via `importNode`). A single plain-HTML
    // root has flag 0 (no trailing argument).
    let mut bits = 0u8;
    if items.len() > 1 {
        bits |= TemplateFlag::FRAGMENT;
    }
    if items_need_import_node(ir, &items) {
        bits |= TemplateFlag::USE_IMPORT_NODE;
    }
    TemplateFactory::FromHtml {
        html,
        fragment_flag: TemplateFlag::from_bits(bits),
    }
}

/// Whether a cleaned region sequence contains an element requiring `importNode`
/// (a `<video>` or a CUSTOM element, anywhere in the rendered subtree) — the
/// official `needs_import_node ||= name === 'video' || is_custom_element`
/// (`RegularElement.js`), which is template-WIDE (any qualifying element sets it).
/// Block bodies are SEPARATE regions (their own factory + flag), so the scan does
/// NOT descend into a block.
fn items_need_import_node(ir: &SvelteRuntimeIr, items: &[CleanItem]) -> bool {
    items.iter().any(|item| match item {
        CleanItem::Node(id) => node_needs_import_node(ir, *id),
        CleanItem::TextRun { .. } => false,
    })
}

/// Whether a node (an element + its rendered element descendants) requires
/// `importNode`. Recurses into element children (the same region), NOT into a
/// block body.
fn node_needs_import_node(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    let IrNode::Element(el) = ir.node(node_id) else {
        return false;
    };
    if is_import_node_element(el) {
        return true;
    }
    el.children.iter().any(|&c| node_needs_import_node(ir, c))
}

/// Whether an element itself requires `importNode`: a `<video>`, or a CUSTOM
/// element (the official `needs_import_node ||= name === 'video' ||
/// is_custom_element`).
fn is_import_node_element(el: &crate::svelte::runtime::ir::ElementIr) -> bool {
    el.tag == "video" || is_custom_element(el)
}

/// Whether an element is a CUSTOM element (the official `is_custom_element_node`):
/// a tag whose name contains `-`, or one bearing an `is="…"` attribute (a
/// customized built-in). A custom element sets its attributes via properties (so
/// they are dropped from the static skeleton, except `is`).
pub(super) fn is_custom_element(el: &crate::svelte::runtime::ir::ElementIr) -> bool {
    el.tag.contains('-')
        || el.attrs.iter().any(|a| {
            matches!(a, AttrIr::Static { name, .. } if name == "is")
                || matches!(a, AttrIr::Dynamic { name, .. } if name == "is")
                || matches!(a, AttrIr::Mixed { name, .. } if name == "is")
        })
}

/// Collect the dynamic slots a region's root nodes (and their descendants) need,
/// tagging each slot with the owning region `scope`. The walk does NOT descend
/// into a nested block body (a `{#each}`/`{#if}`/… body is its OWN region, planned
/// separately) — a block node contributes only its anchor `Block` slot here.
fn collect_slots(
    ir: &SvelteRuntimeIr,
    scope: TemplateScopeId,
    roots: &[NodeId],
    slots: &mut Vec<DynamicSlot>,
) {
    for &root in roots {
        collect_node_slots(ir, scope, root, slots);
    }
}

/// Collect the dynamic slots of one node + descendants, tagged with `scope`.
fn collect_node_slots(
    ir: &SvelteRuntimeIr,
    scope: TemplateScopeId,
    node_id: NodeId,
    slots: &mut Vec<DynamicSlot>,
) {
    // A non-rendering construct (`{@const}` / `{#snippet}` declaration / `{@debug}`
    // / `{@attach}`) occupies no body DOM position and contributes no body slot —
    // its reactive surface (a snippet body, a `{@const}` derived) is a region-local
    // concern the owning backend plans separately, not a body slot.
    if is_non_rendering_node(ir.node(node_id)) {
        return;
    }
    match ir.node(node_id) {
        IrNode::Interpolation { expr, escape, .. } => {
            slots.push(DynamicSlot {
                scope,
                node: node_id,
                kind: DynamicSlotKind::Text {
                    expr: *expr,
                    escape: *escape,
                },
            });
        }
        IrNode::Tag(crate::svelte::runtime::ir::TagIr::Html { expr }) => {
            slots.push(DynamicSlot {
                scope,
                node: node_id,
                kind: DynamicSlotKind::Html { expr: *expr },
            });
        }
        IrNode::Element(el) => {
            collect_attr_slots(scope, node_id, &el.attrs, slots);
            for &child in &el.children {
                collect_node_slots(ir, scope, child, slots);
            }
        }
        IrNode::Block(_) => {
            slots.push(DynamicSlot {
                scope,
                node: node_id,
                kind: DynamicSlotKind::Block,
            });
        }
        IrNode::Component(c) => {
            // A component's dynamic props / binds / spreads are part of the shared
            // SSR dynamic surface — the server renders them just as it does an
            // intrinsic element's dynamic attributes. Collect them, then recurse.
            collect_attr_slots(scope, node_id, &c.attrs, slots);
            for &child in &c.children {
                collect_node_slots(ir, scope, child, slots);
            }
        }
        IrNode::Special(s) => {
            // A special element's OWN dynamic attributes / binds (`<svelte:window
            // bind:innerWidth={w}>`, `<svelte:element this={tag}>`) are part of the
            // shared dynamic surface — collect them.
            collect_attr_slots(scope, node_id, &s.attrs, slots);
            // A NON-BODY special (`<svelte:head>` / window / …) renders its CHILD
            // content in its OWN region (the special-element region-lowering layer
            // plans those region-local slots), so do NOT fold its children into the
            // body slot list (a head `<title>{x}</title>` interpolation is a
            // head-region slot, not a body-DOM slot). A renderable special
            // (`<svelte:element>` / boundary) DOES host body children, so recurse.
            if !is_non_body_special(ir.node(node_id)) {
                for &child in &s.children {
                    collect_node_slots(ir, scope, child, slots);
                }
            }
        }
        IrNode::Text { .. } | IrNode::Comment { .. } | IrNode::Tag(_) => {}
    }
}

/// Collect the dynamic attribute / class / style / spread / bind slots of one
/// element's attributes, targeting `node_id` in region `scope`. Every dynamic
/// attribute surface the server backend renders is represented as a slot (no
/// surface is dropped).
fn collect_attr_slots(
    scope: TemplateScopeId,
    node_id: NodeId,
    attrs: &[AttrIr],
    slots: &mut Vec<DynamicSlot>,
) {
    for attr in attrs {
        let kind = match attr {
            AttrIr::Dynamic { name, expr } => Some(DynamicSlotKind::Attribute {
                name: name.clone(),
                expr: *expr,
            }),
            AttrIr::Mixed { name, parts } => {
                // A mixed value with any expression part is a dynamic attribute
                // surface; collect one Attribute slot per expression part.
                for part in parts {
                    if let crate::svelte::runtime::ir::MixedAttrPart::Expr(expr) = part {
                        slots.push(DynamicSlot {
                            scope,
                            node: node_id,
                            kind: DynamicSlotKind::Attribute {
                                name: name.clone(),
                                expr: *expr,
                            },
                        });
                    }
                }
                None
            }
            AttrIr::Class { name, condition } => Some(DynamicSlotKind::Class {
                name: name.clone(),
                condition: *condition,
            }),
            AttrIr::Style {
                property, value, ..
            } => Some(DynamicSlotKind::Style {
                property: property.clone(),
                // A static-text OR mixed style value carries no SINGLE value EXPRESSION (the
                // dynamic-slot value is one `ExprId`); only a single-expression value carries
                // its id. A mixed value's parts are folded downstream in
                // `project_set_style_op` — the slot here just marks the node dynamic.
                value: match value {
                    StyleDirectiveValue::Expr(e) => Some(*e),
                    StyleDirectiveValue::Text(_) | StyleDirectiveValue::Mixed(_) => None,
                },
            }),
            AttrIr::Spread { expr } => Some(DynamicSlotKind::Spread { expr: *expr }),
            AttrIr::Bind { target, expr } => Some(DynamicSlotKind::Bind {
                target: target.clone(),
                expr: *expr,
            }),
            _ => None,
        };
        if let Some(kind) = kind {
            slots.push(DynamicSlot {
                scope,
                node: node_id,
                kind,
            });
        }
    }
}

/// Build the client-side node-path plans reaching each dynamic node in a region.
///
/// Every emitted [`NodePathPlan`] is SELF-CONTAINED from its [`PathBase`]:
///
/// - A `Fragment` base reaches `roots[i]` via `FirstChild` (descend into the
///   cloned fragment) THEN `Sibling { offset: i }` for `i > 0` — a dynamic root
///   after a static root therefore carries BOTH steps (verified against
///   svelte@5.56.3: `$.sibling($.first_child(fragment), 1)`), never a bare
///   `Sibling` with no descent.
/// - A `Node(parent)` base is emitted ONLY for a parent that has its OWN
///   reachable path — so any element used as a path base is itself named/planned.
///   An element that bears no dynamic attribute but HOSTS a dynamic descendant is
///   given its own path so the descendant's `Node` base is reachable.
///
/// This is the CLIENT-only walk — the server backend ignores it. Every emitted
/// path is tagged with the owning region `scope`; its `PathBase::Fragment` refers
/// to THAT region's own cloned fragment, so a nested block-body region's walk is
/// self-contained. The walk does NOT descend into a nested block body (that body
/// is its own region, planned separately).
fn build_client_paths(
    ir: &SvelteRuntimeIr,
    scope: TemplateScopeId,
    roots: &[NodeId],
    paths: &mut Vec<NodePathPlan>,
) {
    // The cleaned DOM-position sequence (the SAME partition the skeleton emits):
    // each item is ONE DOM node, so its index IS the sibling offset. The region
    // starts from the region-root context (a region's roots are at the fragment
    // level, HTML namespace, never in a `<pre>`).
    let ctx = CleanContext::region_root();
    let items = clean_nodes(ir, roots, ctx);
    // The clone-root contract (`svelte@5.56.3` `Fragment.js` `is_single_element`): a
    // region whose WHOLE cleaned sequence is a SINGLE static-HTML ELEMENT is cloned
    // as that element DIRECTLY (a single-element `$.from_html` returns the element,
    // NOT a fragment). The clone-template VARIABLE *is* that element node — so the
    // element itself needs NO DOM-walk step (a dynamic attr/event on it operates on
    // the clone var directly, the official zero-walk root), and its children descend
    // from the clone-root element via `$.child` (NOT `$.first_child` of a fragment).
    if let [CleanItem::Node(only)] = items.as_slice() {
        if matches!(ir.node(*only), IrNode::Element(_)) && is_static_html_root(ir.node(*only)) {
            build_paths_into_clone_root_element(ir, scope, *only, ctx, paths);
            return;
        }
    }
    // Otherwise the region clones a fragment (multi-root, or a non-element single
    // root such as a block / standalone anchor): a Fragment base reaches position
    // `idx` via FirstChild (descend into the cloned fragment) THEN Sibling{idx} for
    // `idx > 0` — the first hop is `FirstChild`, so `first_from_fragment_is_child` is
    // `false`.
    build_paths_over_items(ir, scope, PathBase::Fragment, &items, ctx, paths, false);
}

/// Build the node-paths INTO a single-element clone-root (the `is_single_element`
/// case): the element node `el` IS the cloned template variable, so it carries NO
/// path of its own; its children descend from the clone-root via `$.child` — the
/// path base stays [`PathBase::Fragment`] (the clone-root) but the FIRST descent
/// step is `Child` (into the clone-root element), matching official's
/// `$.child(root)` (vs `$.first_child(fragment)` of a multi-root fragment).
fn build_paths_into_clone_root_element(
    ir: &SvelteRuntimeIr,
    scope: TemplateScopeId,
    el: NodeId,
    ctx: CleanContext,
    paths: &mut Vec<NodePathPlan>,
) {
    let IrNode::Element(element) = ir.node(el) else {
        return;
    };
    // The element's children descend from the clone-root via `Child` (the clone var
    // is the element). The base remains `Fragment` (it is the region's clone root,
    // which the golden extractor reports as `base: fragment` for a zero-step root),
    // but `first_from_fragment_is_child = true` so the first hop is `Child`.
    let child_ctx = ctx.for_children_of(&element.tag);
    let child_items = clean_nodes(ir, &element.children, child_ctx);
    build_paths_over_items(
        ir,
        scope,
        PathBase::Fragment,
        &child_items,
        child_ctx,
        paths,
        true,
    );
}

/// Build node-paths over a CLEANED DOM-position sequence, rooted at `base`
/// (`Fragment` for a region's roots, `Node(parent)` for an element's children).
/// The first descent step is FirstChild from a Fragment, or Child from a Node;
/// each position's sibling offset is its index in the cleaned sequence.
///
/// For a `TextRun` position that carries interpolations, EACH interpolation node
/// gets a path targeting that position (multiple interps in one run share the DOM
/// text node — the official `flush_sequence` behavior — so they share the offset).
/// For a `Node` position, the node gets a path iff it is itself dynamic OR hosts a
/// dynamic descendant (so it is a reachable `Node` base), and an element node is
/// then descended into over ITS OWN cleaned child sequence.
fn build_paths_over_items(
    ir: &SvelteRuntimeIr,
    scope: TemplateScopeId,
    base: PathBase,
    items: &[CleanItem],
    ctx: CleanContext,
    paths: &mut Vec<NodePathPlan>,
    // Whether the FIRST descent from a `Fragment` base is `FirstChild` (a true
    // multi-root cloned fragment) or `Child` (a single-element clone-root, where the
    // clone var IS the element and we descend into it via `$.child`). A `Node` base
    // always descends via `Child` regardless.
    first_from_fragment_is_child: bool,
) {
    let descend = |idx: usize| -> Vec<NodePathStep> {
        let first = match base {
            PathBase::Fragment if first_from_fragment_is_child => {
                NodePathStep::Child { transparent: false }
            }
            PathBase::Fragment => NodePathStep::FirstChild,
            PathBase::Node(_) => NodePathStep::Child { transparent: false },
        };
        let mut steps = vec![first];
        if idx > 0 {
            steps.push(NodePathStep::Sibling { offset: idx as u32 });
        }
        steps
    };
    for (idx, item) in items.iter().enumerate() {
        match item {
            CleanItem::TextRun { interps, .. } => {
                // Each interpolation sharing this DOM text node gets a path to it.
                for &interp in interps {
                    paths.push(NodePathPlan {
                        scope,
                        node: interp,
                        base,
                        steps: descend(idx),
                    });
                }
            }
            CleanItem::Node(node) => {
                let needs_self = node_needs_path(ir, *node);
                let hosts_dynamic = element_hosts_dynamic_descendant(ir, *node);
                if needs_self || hosts_dynamic {
                    paths.push(NodePathPlan {
                        scope,
                        node: *node,
                        base,
                        steps: descend(idx),
                    });
                }
                // Descend into an element's children over ITS OWN cleaned sequence,
                // so interior sibling offsets index the same positions the skeleton
                // emits. The element now has its own path, so `Node(node)` is a
                // reachable base. The child cleaning context (namespace / whitespace
                // preservation / SVG-`<text>`) matches the skeleton's `clean_nodes`,
                // so the DOM positions (and offsets) stay aligned.
                if let IrNode::Element(el) = ir.node(*node) {
                    let child_ctx = ctx.for_children_of(&el.tag);
                    let child_items = clean_nodes(ir, &el.children, child_ctx);
                    build_paths_over_items(
                        ir,
                        scope,
                        PathBase::Node(*node),
                        &child_items,
                        child_ctx,
                        paths,
                        // A `Node` base always descends via `Child`; the
                        // fragment-first-step flag is irrelevant here.
                        false,
                    );
                }
            }
        }
    }
}

/// Whether an element node HOSTS a dynamic descendant the DOM walk must reach
/// (an interpolation / block / `{@html}` / dynamic-attr-bearing element anywhere
/// in its effective subtree), so the element itself must be a reachable named
/// node (a valid `PathBase::Node`). A non-element node hosts nothing. The
/// element's OWN dynamic attributes are NOT counted here (that is `node_needs_path`
/// / `needs_self`); this is strictly about whether it must serve as a base.
fn element_hosts_dynamic_descendant(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    let IrNode::Element(el) = ir.node(node_id) else {
        return false;
    };
    // The "hosts a dynamic descendant" result is whitespace-invariant (the cleaning
    // context only changes text CONTENT / drop, never which nodes carry interps or
    // are dynamic), so the context passed here does not affect the boolean.
    clean_nodes(ir, &el.children, CleanContext::region_root())
        .iter()
        .any(|item| match item {
            // A TextRun carrying any interpolation is a dynamic descendant.
            CleanItem::TextRun { interps, .. } => !interps.is_empty(),
            CleanItem::Node(c) => {
                node_needs_path(ir, *c) || element_hosts_dynamic_descendant(ir, *c)
            }
        })
}

/// Whether a node needs a client-side path (it is a dynamic node the DOM walk
/// must reach): an interpolation, a block anchor, or an element bearing a dynamic
/// attribute / directive.
fn node_needs_path(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    match ir.node(node_id) {
        IrNode::Interpolation { .. } | IrNode::Block(_) => true,
        IrNode::Element(el) => el.attrs.iter().any(attr_is_dynamic_surface),
        // A raw `{@html}` tag is a dynamic node the DOM walk must reach.
        IrNode::Tag(crate::svelte::runtime::ir::TagIr::Html { .. }) => true,
        _ => false,
    }
}

/// Collect every template-scope id reachable from `scope` (the scope itself plus
/// every block-body / branch template scope nested under its nodes), in
/// IR-traversal order, appending to `out`.
fn collect_template_scopes(
    ir: &SvelteRuntimeIr,
    scope: crate::svelte::runtime::ir::TemplateScopeId,
    out: &mut Vec<crate::svelte::runtime::ir::TemplateScopeId>,
) {
    out.push(scope);
    let roots: Vec<NodeId> = ir.template_scope(scope).roots.clone();
    for node in roots {
        collect_node_template_scopes(ir, node, out);
    }
}

/// Collect the nested template scopes a node introduces (block bodies / branches).
fn collect_node_template_scopes(
    ir: &SvelteRuntimeIr,
    node_id: NodeId,
    out: &mut Vec<crate::svelte::runtime::ir::TemplateScopeId>,
) {
    use crate::svelte::runtime::ir::BlockIr;
    match ir.node(node_id) {
        IrNode::Element(el) => {
            for &child in &el.children {
                collect_node_template_scopes(ir, child, out);
            }
        }
        IrNode::Component(c) => {
            for &child in &c.children {
                collect_node_template_scopes(ir, child, out);
            }
        }
        IrNode::Special(s) => {
            for &child in &s.children {
                collect_node_template_scopes(ir, child, out);
            }
        }
        IrNode::Block(block) => match block {
            BlockIr::If { branches } => {
                for b in branches {
                    collect_template_scopes(ir, b.body, out);
                }
            }
            BlockIr::Each {
                body, else_body, ..
            } => {
                collect_template_scopes(ir, *body, out);
                if let Some(eb) = else_body {
                    collect_template_scopes(ir, *eb, out);
                }
            }
            BlockIr::Await {
                pending,
                then_body,
                catch_body,
                ..
            } => {
                for ts in [pending, then_body, catch_body].into_iter().flatten() {
                    collect_template_scopes(ir, *ts, out);
                }
            }
            BlockIr::Key { body, .. } => collect_template_scopes(ir, *body, out),
            BlockIr::Snippet { body, .. } => collect_template_scopes(ir, *body, out),
        },
        IrNode::Text { .. }
        | IrNode::Comment { .. }
        | IrNode::Interpolation { .. }
        | IrNode::Tag(_) => {}
    }
}

/// Plan the static templates, dynamic slots, and client-side node paths for a
/// component's runtime IR.
///
/// Every template region — the root scope plus every nested block-body / branch
/// scope — contributes its own [`TemplateFactory`]; the regions are emitted in
/// IR-traversal order (root-first). The dynamic slots + client paths are
/// REGION-INDEXED: EVERY region's nodes are walked and each slot / path is tagged
/// with its owning [`TemplateScopeId`], so a dynamic interpolation / bind inside a
/// nested block body is found in the plan (not silently lost). A region's
/// node-path walk is self-contained from THAT region's own cloned fragment.
#[must_use]
pub fn plan_static_templates(ir: &SvelteRuntimeIr) -> StaticTemplatePlan {
    let mut plan = StaticTemplatePlan::default();
    let mut scopes = Vec::new();
    collect_template_scopes(ir, ir.root, &mut scopes);
    for &scope_id in &scopes {
        let scope = ir.template_scope(scope_id);
        // The ROOT region ALWAYS contributes a factory — a zero-root component
        // (only a `<script>` + whitespace, or a fully empty template) still needs a
        // `$.comment()` anchor to mount into. A NESTED empty branch body produces
        // no static template (an empty `{:else}` has nothing to mount). The cleaned
        // sequence is empty iff the region has no rendered DOM position.
        let region_empty = clean_nodes(ir, &scope.roots, CleanContext::region_root()).is_empty();
        if scope_id != ir.root && region_empty {
            continue;
        }
        plan.templates.push(synthesize_region(ir, scope));
    }
    // Walk EVERY region for its dynamic slots + client paths (the nested-block
    // bodies are not reachable from the root region's element-child recursion — a
    // block node contributes only its anchor slot, and its body is a distinct
    // region). For each region:
    //
    // - The body DOM sequence (whitespace-only roots AND non-body specials dropped)
    //   drives the client-side node-path walk: the per-region path sibling offsets
    //   index that region's normalized body skeleton, rooted at the region's own
    //   fragment.
    // - The SLOT sequence keeps non-body specials (whitespace-dropped only) so a
    //   `<svelte:window bind:innerWidth>` dynamic bind is still recorded as a shared
    //   dynamic surface — only its body-DOM walk is omitted.
    for &scope_id in &scopes {
        let region_roots = ir.template_scope(scope_id).roots.clone();
        // The SLOT sequence keeps non-body specials (whitespace-dropped only) so a
        // `<svelte:window bind:innerWidth>` dynamic bind is still recorded as a
        // shared dynamic surface — only its body-DOM walk is omitted.
        let slot_roots: Vec<NodeId> = region_roots
            .iter()
            .copied()
            .filter(|&id| !is_whitespace_text(ir, id))
            .collect();
        collect_slots(ir, scope_id, &slot_roots, &mut plan.slots);
        // The node-path walk keys on the SHARED cleaned DOM-position sequence
        // (`clean_nodes` inside `build_client_paths`), so the per-region path
        // sibling offsets index the same positions the skeleton emits.
        build_client_paths(ir, scope_id, &region_roots, &mut plan.client_paths);
    }
    plan
}
