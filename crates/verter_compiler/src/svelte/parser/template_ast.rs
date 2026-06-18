//! The Svelte template AST.
//!
//! The parser produces a [`ParsedSvelte`] carrying the instance and module
//! `<script>` spans, the component `<style>` span, and a tree of
//! [`SvelteNode`]s covering the FULL current Svelte syntax (Svelte 5.56.x):
//! elements, components, attributes (including Svelte-5 lowercase event
//! attributes and spreads), interpolation, the block constructs
//! (`{#if}`/`{#each}`/`{#await}`/`{#key}`/`{#snippet}`), the rendered-content
//! tags (`{@render}`/`{@html}`/`{@attach}`/`{@const}`/declaration tags
//! `{const}`/`{let}`/`{@debug}`), directive attributes (`bind:`/`class:`/
//! `style:`/`use:`/`transition:`/`in:`/`out:`/`animate:`/legacy `on:`), and
//! the special elements (`<svelte:head>` / `<svelte:element>` / `<svelte:window>`
//! / `<svelte:boundary>` / `<svelte:options>` / `<svelte:component>` /
//! `<svelte:self>` / `<svelte:fragment>`).
//!
//! Every node records spans into the ORIGINAL source so a later projector maps
//! positions precisely. The AST is intentionally LOSSLESS over the matrix: a
//! row's SUPPORTED/OUT-OF-SCOPE disposition is a projector concern, never a
//! parser one — the parser accepts every current-docs construct without crash.

use verter_span::Span;

/// A parsed Svelte component.
///
/// Carries the script region spans (instance + module), the component-level
/// `<style>` span, the template node tree, and any parse diagnostics collected
/// inline. Spans index into the original source.
#[derive(Debug, Clone, Default)]
pub struct ParsedSvelte {
    /// The instance `<script>` block (the default `<script>`), if present.
    pub instance_script: Option<SvelteScript>,
    /// The module `<script module>` (or legacy `<script context="module">`)
    /// block, if present.
    pub module_script: Option<SvelteScript>,
    /// The component-level `<style>` blocks (opaque content; CSS domain).
    pub styles: Vec<SvelteStyle>,
    /// The template node tree (everything outside the script/style blocks).
    pub template: Vec<SvelteNode>,
    /// Parse diagnostics collected inline (never a hard failure).
    pub diagnostics: Vec<SvelteParseDiagnostic>,
}

impl ParsedSvelte {
    /// The instance-script content span, if an instance `<script>` is present.
    #[must_use]
    pub fn instance_content(&self) -> Option<Span> {
        self.instance_script.as_ref().and_then(|s| s.content)
    }

    /// The module-script content span, if a `<script module>` is present.
    #[must_use]
    pub fn module_content(&self) -> Option<Span> {
        self.module_script.as_ref().and_then(|s| s.content)
    }
}

/// The explicit reactivity-mode forced by a top-level `<svelte:options runes={…}>`
/// element (Svelte's own forced-mode switch). Returns `Some(true)` for `runes` /
/// `runes={true}`, `Some(false)` for `runes={false}`, and `None` when no
/// `<svelte:options runes>` is present (the caller then falls back to rune-USAGE
/// detection).
///
/// Only a TOP-LEVEL options element counts (Svelte requires `<svelte:options>` at
/// the component root). This is the SINGLE shared syntactic query both the IDE TSX
/// projector (legacy-mode classification) and the runtime-IR mode inference
/// consume — the parse-tree types are owned here, so the query lives here rather
/// than being forked per surface.
#[must_use]
pub fn forced_runes_option(source: &str, nodes: &[SvelteNode]) -> Option<bool> {
    for node in nodes {
        if let SvelteNode::Element(el) = node {
            if matches!(
                el.kind,
                SvelteElementKind::Special(SvelteSpecialKind::Options)
            ) {
                if let Some(v) = runes_option_value(source, el) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// The `runes` option value on a `<svelte:options>` element: a valueless `runes`
/// boolean-shorthand is `true`; `runes={true}` / `runes={false}` read the literal;
/// any other form is treated as absent (`None`).
fn runes_option_value(source: &str, el: &SvelteElement) -> Option<bool> {
    el.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain { name, value, .. } if name == "runes" => match value {
            // `runes` (no value) — boolean shorthand ⇒ true.
            None => Some(true),
            // `runes={true}` / `runes={false}` — read the expression literal.
            Some(SvelteAttributeValue::Expression(span)) => {
                let text = source[span.start as usize..span.end as usize].trim();
                match text {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                }
            }
            _ => None,
        },
        _ => None,
    })
}

/// One `<script>` block (instance or module).
#[derive(Debug, Clone)]
pub struct SvelteScript {
    /// Whether this is the module script (`<script module>` /
    /// `<script context="module">`).
    pub is_module: bool,
    /// The full open-tag span (`<script ...>`).
    pub tag_open: Span,
    /// The script content span (between the open and close tags), if any.
    pub content: Option<Span>,
    /// The raw attribute spans on the open tag.
    pub attributes: Vec<SvelteAttribute>,
    /// The `lang` attribute value, if present (`ts` / `tsx` / …).
    pub lang: Option<String>,
}

/// One component-level `<style>` block — opaque content (CSS domain).
#[derive(Debug, Clone)]
pub struct SvelteStyle {
    /// The full open-tag span.
    pub tag_open: Span,
    /// The style content span, if any.
    pub content: Option<Span>,
    /// The raw attribute spans on the open tag.
    pub attributes: Vec<SvelteAttribute>,
}

/// One template node.
#[derive(Debug, Clone)]
pub enum SvelteNode {
    /// Literal text run.
    Text(Span),
    /// `<!-- ... -->` comment.
    Comment(Span),
    /// A `{expr}` interpolation. The span covers the inner expression (excludes
    /// the braces).
    Interpolation(Span),
    /// A regular element, component, or special element.
    Element(SvelteElement),
    /// A block construct (`{#if}` / `{#each}` / `{#await}` / `{#key}` /
    /// `{#snippet}`).
    Block(SvelteBlock),
    /// A standalone tag (`{@render}` / `{@html}` / `{@const}` / `{@debug}` /
    /// `{@attach}` / declaration tags `{const}` / `{let}`).
    Tag(SvelteTag),
}

/// A template element / component / special element.
#[derive(Debug, Clone)]
pub struct SvelteElement {
    /// The tag name (`div`, `MyComponent`, `svelte:head`, …).
    pub name: String,
    /// The span of the tag name in the source.
    pub name_span: Span,
    /// The element's structural kind.
    pub kind: SvelteElementKind,
    /// The attributes / directives on the open tag.
    pub attributes: Vec<SvelteAttribute>,
    /// The child nodes (empty for self-closing / void elements).
    pub children: Vec<SvelteNode>,
    /// Whether the element was self-closing (`<x />`).
    pub self_closing: bool,
    /// The full open-tag span.
    pub open_span: Span,
    /// The full span of the MATCHING `</name>` close tag — `start` at the `<` of
    /// `</name`, `end` just past the closing `>`. `None` for a self-closing or
    /// unterminated element. Recorded by the string/brace-aware child walk (the
    /// parser is the close-tag authority); consumers read this instead of
    /// re-deriving the close tag with a literal-unaware source scan.
    pub close_span: Option<Span>,
}

/// The structural kind of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvelteElementKind {
    /// A lowercase intrinsic HTML element (`div`, `span`, …).
    Intrinsic,
    /// An uppercase or dotted component reference (`Foo`, `Foo.Bar`).
    Component,
    /// A `<svelte:*>` special element, carrying its closed kind.
    Special(SvelteSpecialKind),
    /// A nested `<style>` element inside template markup (opaque, CSS domain).
    NestedStyle,
}

/// The closed family of `<svelte:*>` special elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteSpecialKind {
    /// `<svelte:head>`.
    Head,
    /// `<svelte:window>`.
    Window,
    /// `<svelte:document>`.
    Document,
    /// `<svelte:body>`.
    Body,
    /// `<svelte:element this={...}>`.
    Element,
    /// `<svelte:boundary>`.
    Boundary,
    /// `<svelte:options>`.
    Options,
    /// `<svelte:component this={C}>` (dynamic component, F8).
    Component,
    /// `<svelte:self>` (recursive self reference, F8).
    SelfRef,
    /// `<svelte:fragment slot="x">` (transparent slot-grouping fragment, F9).
    Fragment,
    /// An unrecognised `<svelte:*>` name — parsed without crash.
    Unknown,
}

impl SvelteSpecialKind {
    /// Classify a `svelte:<local>` special-element local name.
    #[must_use]
    pub fn from_local(local: &str) -> Self {
        match local {
            "head" => Self::Head,
            "window" => Self::Window,
            "document" => Self::Document,
            "body" => Self::Body,
            "element" => Self::Element,
            "boundary" => Self::Boundary,
            "options" => Self::Options,
            "component" => Self::Component,
            "self" => Self::SelfRef,
            "fragment" => Self::Fragment,
            _ => Self::Unknown,
        }
    }
}

/// One attribute or directive on an element open tag.
#[derive(Debug, Clone)]
pub struct SvelteAttribute {
    /// The attribute kind (plain / directive / spread / …).
    pub kind: SvelteAttributeKind,
    /// The full attribute span (name + value).
    pub span: Span,
}

/// The closed family of attribute / directive kinds.
///
/// Every current-docs attribute and directive form is represented; a row's
/// SUPPORTED/OUT-OF-SCOPE status is a projector concern, not a parser one.
#[derive(Debug, Clone)]
pub enum SvelteAttributeKind {
    /// A plain attribute (`class="x"`, `id={expr}`, shorthand `{value}`,
    /// CSS custom property `--name={expr}`). Carries the name and the optional
    /// value span (an interpolation value span excludes braces).
    Plain {
        /// The attribute name (e.g. `class`, `onclick`, `--accent`).
        name: String,
        /// The name span.
        name_span: Span,
        /// The value span (string body or `{expr}` inner), if present.
        value: Option<SvelteAttributeValue>,
    },
    /// A spread attribute (`{...rest}`). Carries the inner-expression span.
    Spread(Span),
    /// A directive attribute (`bind:`, `class:`, `style:`, `use:`,
    /// `transition:`/`in:`/`out:`, `animate:`, legacy `on:`).
    Directive(SvelteDirective),
}

/// An attribute value (a quoted string body or an interpolation expression).
#[derive(Debug, Clone)]
pub enum SvelteAttributeValue {
    /// A quoted string value body (span excludes the quotes).
    Text(Span),
    /// A single `{expr}` value (span excludes the braces).
    Expression(Span),
    /// A mixed/concatenated value (string + interpolation runs) — the whole
    /// value span is recorded; the parser does not split the runs.
    Mixed(Span),
}

/// A directive attribute.
#[derive(Debug, Clone)]
pub struct SvelteDirective {
    /// The directive kind.
    pub kind: SvelteDirectiveKind,
    /// The directive's local name (the part after the `:`, before any
    /// modifiers) — e.g. `value` in `bind:value`, `click` in `on:click`.
    pub local: String,
    /// The `|modifier` list (e.g. `|important`, `|local`, `|stop`).
    pub modifiers: Vec<String>,
    /// The value expression span (`{expr}` inner or quoted body), if present.
    /// A two-expression function binding `bind:x={get, set}` records the whole
    /// inner span (both expressions); the projector splits it.
    pub value: Option<SvelteAttributeValue>,
}

/// The closed family of directive kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteDirectiveKind {
    /// `bind:` two-way binding (incl. function-binding `bind:x={get, set}`).
    Bind,
    /// `class:` conditional class.
    Class,
    /// `style:` inline style (+ `|important`).
    Style,
    /// `use:` action.
    Use,
    /// `transition:`.
    Transition,
    /// `in:` (one-way transition in).
    In,
    /// `out:` (one-way transition out).
    Out,
    /// `animate:`.
    Animate,
    /// Legacy `on:` event listener.
    On,
    /// `let:` slot-prop binding (`<C let:item={alias}>` / shorthand `let:item`).
    Let,
    /// An unrecognised `name:` directive — parsed without crash.
    Unknown,
}

impl SvelteDirectiveKind {
    /// Classify a directive prefix (the part before the first `:`).
    #[must_use]
    pub fn from_prefix(prefix: &str) -> Self {
        match prefix {
            "bind" => Self::Bind,
            "class" => Self::Class,
            "style" => Self::Style,
            "use" => Self::Use,
            "transition" => Self::Transition,
            "in" => Self::In,
            "out" => Self::Out,
            "animate" => Self::Animate,
            "on" => Self::On,
            "let" => Self::Let,
            _ => Self::Unknown,
        }
    }
}

/// A block construct.
#[derive(Debug, Clone)]
pub struct SvelteBlock {
    /// The block kind.
    pub kind: SvelteBlockKind,
    /// The full block span (open tag through the closing `{/...}`).
    pub span: Span,
    /// The block's primary head expression span (the `{#if expr}` condition,
    /// the `{#each list as item}` list, the `{#await expr}` promise, the
    /// `{#key expr}` key). `None` for `{#snippet}` (its head is the name +
    /// params, recorded on the snippet kind).
    pub head_expr: Option<Span>,
    /// The block's children (the immediate body run).
    pub children: Vec<SvelteNode>,
    /// Branch clauses (`{:else if}` / `{:else}` / `{:then}` / `{:catch}`).
    pub clauses: Vec<SvelteBlockClause>,
}

/// The closed family of block kinds.
#[derive(Debug, Clone)]
pub enum SvelteBlockKind {
    /// `{#if expr}` … `{/if}`.
    If,
    /// `{#each list as item, index (key)}` … `{/each}`. Records the `as`
    /// binding span (absent for the `{#each {length: n}}` no-item form), the
    /// optional index binding span, and the optional `(key)` span.
    Each {
        /// The `as <pattern>` binding span, if present.
        item: Option<Span>,
        /// The `, <index>` binding span, if present.
        index: Option<Span>,
        /// The `(<key>)` expression span, if present.
        key: Option<Span>,
    },
    /// `{#await expr}` … `{:then v}` … `{:catch e}` … `{/await}`.
    Await {
        /// The `{:then <pattern>}` binding span, if present.
        then_binding: Option<Span>,
        /// The `{:catch <pattern>}` binding span, if present.
        catch_binding: Option<Span>,
    },
    /// `{#key expr}` … `{/key}`.
    Key,
    /// `{#snippet name(params)}` … `{/snippet}`. Records the name span and the
    /// parameter list span.
    Snippet {
        /// The snippet name span.
        name: Span,
        /// The snippet name as text.
        name_text: String,
        /// The `(params)` span (excludes the parens), if present.
        params: Option<Span>,
    },
}

/// One branch clause of a block.
#[derive(Debug, Clone)]
pub struct SvelteBlockClause {
    /// The clause kind.
    pub kind: SvelteClauseKind,
    /// The clause's expression/binding span, if any (the `{:else if expr}`
    /// condition, the `{:then v}` binding).
    pub expr: Option<Span>,
    /// The clause-tag head span — the whole `{:else}` / `{:else if d}` /
    /// `{:then v}` / `{:catch e}` head INCLUDING the braces. The projector
    /// OVERWRITES this span directly (no source reverse-scan), so an empty
    /// clause (`{:else}` / `{:then}` / `{:catch}` with no expr and no children)
    /// is still rewritten and never leaks raw `{:…}` into the projected TSX.
    pub tag_span: Span,
    /// The clause's children.
    pub children: Vec<SvelteNode>,
}

/// The closed family of clause kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteClauseKind {
    /// `{:else if expr}`.
    ElseIf,
    /// `{:else}`.
    Else,
    /// `{:then v}`.
    Then,
    /// `{:catch e}`.
    Catch,
}

/// A standalone tag.
#[derive(Debug, Clone)]
pub struct SvelteTag {
    /// The tag kind.
    pub kind: SvelteTagKind,
    /// The full tag span.
    pub span: Span,
    /// The tag's inner expression / declaration span (excludes the braces and
    /// the leading keyword).
    pub inner: Span,
}

/// The closed family of standalone-tag kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteTagKind {
    /// `{@render snippet(args)}`.
    Render,
    /// `{@html expr}`.
    Html,
    /// `{@const x = expr}` (documented legacy since 5.56).
    LegacyConst,
    /// `{const x = expr}` (5.56 declaration tag).
    Const,
    /// `{let x = expr}` (5.56 declaration tag).
    Let,
    /// `{@debug var1, var2}`.
    Debug,
    /// `{@attach expr}` (5.29).
    Attach,
    /// An unrecognised `{@name ...}` tag — parsed without crash.
    Unknown,
}

/// One inline parse diagnostic.
#[derive(Debug, Clone)]
pub struct SvelteParseDiagnostic {
    /// A short machine-stable code (e.g. `unterminated-block`).
    pub code: &'static str,
    /// A human-readable message.
    pub message: String,
    /// The diagnostic span.
    pub span: Span,
}
