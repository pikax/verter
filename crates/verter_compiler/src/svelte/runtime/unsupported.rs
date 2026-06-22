//! The closed family of unsupported Svelte runtime surfaces + its diagnostic
//! projection (owning vertical, diagnostic id, message, span). Each later block
//! deletes its own arm + tests cleanly.

use verter_span::Span;

/// The closed family of Svelte runtime surfaces this backend does NOT yet emit.
///
/// Each variant names ONE surface family and records its OWNING vertical (the
/// later block that lands it), so the fail-closed diagnostics group by owner and
/// each later block deletes its own arm + tests cleanly. The diagnostic id shape
/// is `svelte-runtime-unsupported-<surface>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedSvelteRuntimeSurface {
    /// A dynamic attribute, boolean DOM prop, `class:` / `class={}`, `style:` /
    /// `style={}`, or a non-static property write not covered by the input
    /// cleanup (5a).
    DynamicAttribute {
        /// The attribute / directive name.
        name: String,
        /// The source span.
        span: Span,
    },
    /// A spread `{...x}` or `{@html}` (5b).
    SpreadOrHtml {
        /// The source span.
        span: Span,
    },
    /// A binding beyond `bind:value` on an `<input>` and element `bind:this` (5c).
    Binding {
        /// The bind target (`checked`, `group`, `this` on a component, …).
        target: String,
        /// The source span.
        span: Span,
    },
    /// A non-delegated / capture / global-target event, or a legacy `on:`
    /// modifier wrapper (5d).
    NonDelegatedEvent {
        /// The event type.
        event_type: String,
        /// The source span.
        span: Span,
    },
    /// A control-flow block (`{#if}` / `{#each}` / `{#await}` / `{#key}`) or a
    /// declaration / `{@const}` / `{@debug}` tag (5e).
    Block {
        /// A short construct label (`if`, `each`, `const`, …).
        construct: &'static str,
        /// The source span.
        span: Span,
    },
    /// A component, snippet, `{@render}`, `{@attach}`, transition, action,
    /// animation, or a renderable `<svelte:*>` element (5f).
    ComponentOrSnippet {
        /// A short construct label.
        construct: &'static str,
        /// The source span.
        span: Span,
    },
    /// A rune beyond the supported `$state` / `$derived` / `$effect` / basic
    /// `$props()` subset (`$state.raw` / `$state.snapshot` / `$effect.pre` /
    /// `$effect.root` / `$effect.tracking` / `$props()` rest / `$props.id` /
    /// `$bindable` / `$inspect`) (5g).
    AdvancedRune {
        /// A short rune label.
        rune: &'static str,
        /// The source span.
        span: Span,
    },
    /// A `$host()` rune (a custom-element-only API) / custom-element output (5h).
    HostOrCustomElement {
        /// A short rune / surface label.
        surface: &'static str,
        /// The source span.
        span: Span,
    },
    /// An intrinsic element whose tag is NOT in the finite client-core element
    /// allowlist ([`SupportedHtmlElement`](super::client_allowlist::SupportedHtmlElement)
    /// — `<a>` / `<button>` / `<div>` / `<h1>` / `<input>` / `<p>`). The client core
    /// emits ONLY that finite set; EVERY other intrinsic HTML tag (`<span>` /
    /// `<section>` / `<ul>` / `<textarea>` / `<select>` / `<option>` / `<img>` /
    /// `<video>` / a raw `<slot>` / a special-content-model tag …) fails closed here.
    /// Adding a tag requires extending the finite enum AND adding a golden in the same
    /// change. This is the "regular element not yet in the client core" refusal (5a),
    /// kept DISTINCT from [`ElementName`](Self::ElementName) (a tag that IS in the
    /// allowlist-shape but is an invalid/reserved JS binding name).
    Element {
        /// The out-of-allowlist tag name.
        tag: String,
        /// The source span.
        span: Span,
    },
    /// An element whose tag would synthesize an INVALID / RESERVED JS local var name
    /// (`<var>` → `var var = root();`, `<class>` → `var class = root();`, or any tag
    /// not matching `/^[A-Za-z_$][A-Za-z0-9_$]*$/`). The official compiler
    /// collision-renames the DOM local (`var_1` / `class_1`) through its general name
    /// allocator; that naming-completion path is a deferral-ledger follow-up, so an
    /// identifier-unsafe element tag fails closed rather than emitting invalid JS (5v).
    ElementName {
        /// The offending tag name.
        tag: String,
        /// The source span.
        span: Span,
    },
    /// A NON-REACTIVE interpolation (`{1 + 1}` / `{moduleConst}` / a never-written
    /// `$state` read) whose value never changes. The official compiler STATIC-FOLDS
    /// it to a `node.textContent = '…'` write rather than a reactive
    /// `$.template_effect`; static folding is a deferral-ledger follow-up, so a
    /// non-reactive interpolation fails closed rather than emitting a divergent
    /// reactive-text op (5n).
    StaticInterpolation {
        /// The source span of the interpolation expression.
        span: Span,
    },
    /// A DESTRUCTURING assignment / update target outside the supported lvalue
    /// subset (`({ count } = obj)` / `[count] = arr`). The official compiler lowers
    /// it through a destructure closure (`$.set(count, obj.count, true)`); that
    /// lowering is a deferral-ledger follow-up, so a destructuring write target
    /// fails closed rather than emitting a raw (un-rewritten) destructuring
    /// assignment to a reactive binding (5p).
    DestructuringWrite {
        /// The source span.
        span: Span,
    },
    /// A ROOT TEXT-NODE region — a bare reactive interpolation (`{count}`) as the
    /// component ROOT, with no wrapping element. The official compiler emits the
    /// text-first topology (a `$.text()` root reached via `$.next()`, then a
    /// `$.template_effect` over it) — a distinct emission shape from the
    /// `from_html`-clone path. That topology is a deferral-ledger follow-up, so a
    /// root reactive text-node region fails closed rather than emitting an
    /// undeclared text-node var (invalid JS) (5q).
    RootTextRegion {
        /// The source span of the root interpolation region.
        span: Span,
    },
    /// Legacy non-runes lowering (`export let` / `$:` / `<slot>` / store
    /// auto-subscriptions / the legacy flag) (5i).
    LegacyMode {
        /// The source span.
        span: Span,
    },
    /// Experimental async (`$state.eager` / `$effect.pending` / async block
    /// helpers / async `$derived`) (5j).
    ExperimentalAsync {
        /// A short rune / surface label.
        surface: &'static str,
        /// The source span.
        span: Span,
    },
    /// A dev-mode (`dev: true`) codegen request — the dev-mode output axis
    /// (validation wrappers, `$.add_locations`, dev `$inspect` / `$.trace`) is not
    /// emitted; only the production runes client output is (5k).
    DevMode {
        /// The source span.
        span: Span,
    },
    /// A top-level `<style>` / CSS scoping surface (5l).
    Style {
        /// The source span.
        span: Span,
    },
    /// `<svelte:options>` / a compile-option axis beyond name/runes/client (5m).
    OptionsAxis {
        /// The source span.
        span: Span,
    },
    /// A reactive interpolation whose expression is NOT a bare identifier read
    /// (a binary / logical / conditional / call / optional-call / member /
    /// sequence / unary / `new` / template / assignment expression). The supported
    /// interpolation surface is a bare `Identifier` resolving to a reactive
    /// `$state` signal read or a no-default prop read; every other expression shape
    /// needs the official `build_template_chunk` evaluator (memoizer / `has_call` /
    /// nullish-coalesce / parenthesization) — a reactive-text-completion follow-up,
    /// so it fails closed (5r).
    ComplexInterpolation {
        /// The source span of the interpolation expression.
        span: Span,
    },
    /// An instance-script `import` or a `<script module>` script. The official
    /// compiler hoists an instance import to module scope and emits a module-script
    /// statement; that script-hoisting lowering is a script-completion follow-up,
    /// so an import / module script fails closed (5s).
    ScriptImport {
        /// A short construct label (`import`, `module script`).
        construct: &'static str,
        /// The source span.
        span: Span,
    },
    /// A `<script lang="ts">` / `<script lang="tsx">` (TypeScript) script. The
    /// official compiler strips the TS annotations before lowering; the TS-strip
    /// path is a script-completion follow-up, so a TypeScript script fails closed
    /// (5t).
    TypeScript {
        /// The source span.
        span: Span,
    },
    /// A reactive-text run whose LITERAL chunk is not simple ASCII (an HTML entity
    /// reference, a tab / newline / repeated-space run, or a character needing
    /// template-literal escaping / whitespace normalization). The supported literal
    /// chunk is simple ASCII; a complex chunk needs the official boundary-trimming /
    /// entity-decode / escaping path — a reactive-text-completion follow-up, so it
    /// fails closed (5u).
    ComplexTextChunk {
        /// The source span of the text run.
        span: Span,
    },
    /// A TOP-LEVEL instance-script item that is NOT one of the three supported
    /// declaration shapes (a `let name = $state(<primitive literal>)` declarator, a
    /// single no-default `$props()` destructure, or a bare `let el;` used solely as a
    /// `bind:this` target). The supported instance script is a STRICT FINITE
    /// ALLOWLIST: every OTHER top-level item — a top-level function / class / enum /
    /// namespace / interface / type alias in a plain `<script>`, a plain non-rune
    /// `let` / `const` / `var`, an arbitrary expression / control-flow / empty
    /// statement, a `$:` reactive label, or a `$` / `$$`-prefixed binding — fails
    /// closed here BY CONSTRUCTION rather than reaching the broad statement-rewrite
    /// lowering. Adding a shape requires extending the finite
    /// [`SupportedInstanceScriptItem`](super::client_shapes::SupportedInstanceScriptItem)
    /// enum AND adding a golden in the same change. The script-completion vertical
    /// (5w) owns the broader instance-script surface (functions, statements,
    /// `$:` reactivity, multi-declarators).
    InstanceScriptItem {
        /// A short construct label (`function`, `class`, `enum`, `$: label`,
        /// `plain let`, `$$-prefixed binding`, …).
        construct: &'static str,
        /// The source span.
        span: Span,
    },
    /// A compiler-MAGIC identifier (`$$slots`, `$$props`, `$$restProps`) referenced in
    /// the instance script or a template expression. These are auto-injected legacy
    /// magic objects the official compiler synthesizes from the component signature
    /// (`$$slots` from the slot inventory, `$$props` / `$$restProps` from the props
    /// object); emitting a raw reference to them in the runes client output binds an
    /// undefined identifier (a runtime `ReferenceError`). The magic-object synthesis
    /// is a script-completion follow-up (5w), so a magic-identifier reference fails
    /// closed rather than emitting an undefined read.
    MagicIdentifier {
        /// The magic identifier name (`$$slots` / `$$props` / `$$restProps`).
        name: &'static str,
        /// The source span.
        span: Span,
    },
    /// A `<p>` with a DIRECT disallowed block child (`<div>` / `<h1>` / `<p>` …) but NO
    /// surviving explicit `</p>` close — the IMPLICIT-autoclose case. The official
    /// compiler AUTO-CLOSES the `<p>` (a warning) and re-parents the block element as a
    /// sibling, then ACCEPTS the component; modeling that autoclose DOM re-parenting is
    /// outside the §1.2 core, so it fails closed as an unsupported FEATURE (5x) rather
    /// than emitting the wrong DOM tree. (The EXPLICIT-`</p>` case is an official REJECT
    /// — `element_invalid_closing_tag_autoclosed` — handled by the official-reject gate.)
    ParagraphAutoclose {
        /// The block child tag that triggers the autoclose (`div` / `h1` / `p` / …).
        child: String,
        /// The source span (the auto-closed `<p>`'s open tag).
        span: Span,
    },
    /// A constant-foldable mixed-attribute interpolation whose Svelte `Evaluation`
    /// would call a native JS operation that THROWS at compile time (the official
    /// compiler compile-FAILS the component) — OR whose throw status Verter cannot
    /// prove non-throwing. The const-fold tri-state contract's `Refuse` arm: a
    /// DETERMINISTIC compile refusal, NEVER emitted as live code (emitting the live
    /// expression would convert the official compiler's compile-failure into a
    /// runtime crash). This is the EAGER `Evaluation` semantics — a throw in a
    /// non-selected logical operand / conditional branch (`false && (1n / 0n)`)
    /// still refuses, because official evaluates both before selecting (5a). Cases:
    /// mixing BigInt with a Number in arithmetic / bitwise (`2 + 1n`), BigInt
    /// division / remainder by `0n`, BigInt `>>>`, unary `+` on BigInt, a negative
    /// BigInt exponent, `in` / `instanceof` with a known primitive RHS, and a
    /// foldable global throwing under known args (`Math.clz32(1n)`,
    /// `String.fromCodePoint(-1 | 1.5 | 0x110000)`).
    ConstFoldThrow {
        /// A short, deterministic reason label (NOT V8's error text) — the throwing
        /// construct (`bigint mixed with number in arithmetic`, `bigint division by
        /// zero`, …).
        reason: &'static str,
        /// The source span of the interpolation expression.
        span: Span,
    },
    /// SSR (`generate: 'server'`) — the server backend is not yet implemented
    /// (owning vertical 8).
    ServerGenerate {
        /// The source span.
        span: Span,
    },
}

impl UnsupportedSvelteRuntimeSurface {
    /// The owning vertical label (the block that lands this surface).
    #[must_use]
    pub fn owning_block(&self) -> &'static str {
        match self {
            Self::DynamicAttribute { .. } => "5a",
            Self::SpreadOrHtml { .. } => "5b",
            Self::Binding { .. } => "5c",
            Self::NonDelegatedEvent { .. } => "5d",
            Self::Block { .. } => "5e",
            Self::ComponentOrSnippet { .. } => "5f",
            Self::AdvancedRune { .. } => "5g",
            Self::HostOrCustomElement { .. } => "5h",
            // An out-of-allowlist intrinsic element is the regular-element owner (the
            // same vertical owning the form / special-element + input-cleanup breadth).
            Self::Element { .. } => "5a",
            Self::ElementName { .. } => "5v",
            Self::LegacyMode { .. } => "5i",
            Self::ExperimentalAsync { .. } => "5j",
            Self::DevMode { .. } => "5k",
            Self::Style { .. } => "5l",
            Self::OptionsAxis { .. } => "5m",
            Self::StaticInterpolation { .. } => "5n",
            Self::DestructuringWrite { .. } => "5p",
            Self::RootTextRegion { .. } => "5q",
            Self::ComplexInterpolation { .. } => "5r",
            Self::ScriptImport { .. } => "5s",
            Self::TypeScript { .. } => "5t",
            Self::ComplexTextChunk { .. } => "5u",
            // A non-allowlist instance-script item / a magic identifier is the
            // script-completion vertical (functions, statements, `$:` reactivity,
            // the auto-injected `$$slots`/`$$props`/`$$restProps` magic objects).
            Self::InstanceScriptItem { .. } => "5w",
            Self::MagicIdentifier { .. } => "5w",
            // The `<p>` implicit-autoclose DOM re-parenting is the close-tag-structure
            // completion vertical (5x).
            Self::ParagraphAutoclose { .. } => "5x",
            // A const-fold compile-time throw is a 5a mixed-attribute surface (the
            // const-fold tri-state `Refuse` arm).
            Self::ConstFoldThrow { .. } => "5a",
            Self::ServerGenerate { .. } => "8",
        }
    }

    /// The machine-stable diagnostic id (`svelte-runtime-unsupported-<surface>`).
    #[must_use]
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::DynamicAttribute { .. } => "svelte-runtime-unsupported-dynamic-attribute",
            Self::SpreadOrHtml { .. } => "svelte-runtime-unsupported-spread-or-html",
            Self::Binding { .. } => "svelte-runtime-unsupported-binding",
            Self::NonDelegatedEvent { .. } => "svelte-runtime-unsupported-non-delegated-event",
            Self::Block { .. } => "svelte-runtime-unsupported-block",
            Self::ComponentOrSnippet { .. } => "svelte-runtime-unsupported-component",
            Self::AdvancedRune { .. } => "svelte-runtime-unsupported-advanced-rune",
            Self::HostOrCustomElement { .. } => "svelte-runtime-unsupported-host-custom-element",
            Self::Element { .. } => "svelte-runtime-unsupported-element",
            Self::ElementName { .. } => "svelte-runtime-unsupported-element-name",
            Self::LegacyMode { .. } => "svelte-runtime-unsupported-legacy-mode",
            Self::ExperimentalAsync { .. } => "svelte-runtime-unsupported-experimental-async",
            Self::DevMode { .. } => "svelte-runtime-unsupported-dev-mode",
            Self::Style { .. } => "svelte-runtime-unsupported-style",
            Self::OptionsAxis { .. } => "svelte-runtime-unsupported-options",
            Self::StaticInterpolation { .. } => "svelte-runtime-unsupported-static-interpolation",
            Self::DestructuringWrite { .. } => "svelte-runtime-unsupported-destructuring-write",
            Self::RootTextRegion { .. } => "svelte-runtime-unsupported-root-text-region",
            Self::ComplexInterpolation { .. } => "svelte-runtime-unsupported-complex-interpolation",
            Self::ScriptImport { .. } => "svelte-runtime-unsupported-script-import",
            Self::TypeScript { .. } => "svelte-runtime-unsupported-typescript",
            Self::ComplexTextChunk { .. } => "svelte-runtime-unsupported-complex-text",
            Self::InstanceScriptItem { .. } => "svelte-runtime-unsupported-instance-script-item",
            Self::MagicIdentifier { .. } => "svelte-runtime-unsupported-magic-identifier",
            Self::ParagraphAutoclose { .. } => "svelte-runtime-unsupported-paragraph-autoclose",
            Self::ConstFoldThrow { .. } => "svelte-runtime-unsupported-const-fold-throw",
            Self::ServerGenerate { .. } => "svelte-runtime-unsupported-server-generate",
        }
    }

    /// A human-readable message naming the surface + owning vertical.
    #[must_use]
    pub fn message(&self) -> String {
        let detail = match self {
            Self::DynamicAttribute { name, .. } => {
                format!("the dynamic attribute / directive `{name}`")
            }
            Self::SpreadOrHtml { .. } => "a spread `{...}` / `{@html}` surface".to_string(),
            Self::Binding { target, .. } => format!("the `bind:{target}` binding"),
            Self::NonDelegatedEvent { event_type, .. } => {
                format!("the non-delegated / capture / global event `{event_type}`")
            }
            Self::Block { construct, .. } => format!("the `{construct}` block construct"),
            Self::ComponentOrSnippet { construct, .. } => format!("the `{construct}` construct"),
            Self::AdvancedRune { rune, .. } => format!("the `{rune}` rune form"),
            Self::HostOrCustomElement { surface, .. } => {
                format!("the `{surface}` custom-element surface")
            }
            Self::Element { tag, .. } => format!(
                "the `<{tag}>` element (it is not in the finite client-core element \
                 allowlist `a` / `button` / `div` / `h1` / `input` / `p`)"
            ),
            Self::ElementName { tag, .. } => format!(
                "the `<{tag}>` element (its synthesized DOM local var name would be an \
                 invalid / reserved JS identifier; the official compiler collision-renames it)"
            ),
            Self::LegacyMode { .. } => "legacy (non-runes) mode".to_string(),
            Self::ExperimentalAsync { surface, .. } => {
                format!("the experimental-async `{surface}` surface")
            }
            Self::DevMode { .. } => "dev-mode (`dev: true`) codegen".to_string(),
            Self::Style { .. } => "a `<style>` / CSS-scoping surface".to_string(),
            Self::OptionsAxis { .. } => "a `<svelte:options>` / compile-option surface".to_string(),
            Self::StaticInterpolation { .. } => {
                "a non-reactive interpolation (the official compiler static-folds it to a \
                 `textContent` write)"
                    .to_string()
            }
            Self::DestructuringWrite { .. } => {
                "a destructuring assignment / update target".to_string()
            }
            Self::RootTextRegion { .. } => {
                "a root text-node region (a bare reactive interpolation as the component root; \
                 the official compiler emits the text-first `$.text()` / `$.next()` topology)"
                    .to_string()
            }
            Self::ComplexInterpolation { .. } => {
                "a non-identifier reactive interpolation (a binary / logical / \
                 conditional / call / member / `new` / template expression; the \
                 official compiler routes it through `build_template_chunk`)"
                    .to_string()
            }
            Self::ScriptImport { construct, .. } => {
                format!("a `{construct}` (the official compiler hoists it to module scope)")
            }
            Self::TypeScript { .. } => {
                "a `<script lang=\"ts\">` TypeScript script (the official compiler strips \
                 the TS annotations before lowering)"
                    .to_string()
            }
            Self::ComplexTextChunk { .. } => {
                "a reactive-text run with a non-simple-ASCII literal chunk (an HTML \
                 entity, a tab / newline / repeated-space run, or an escaping / \
                 whitespace-normalization need)"
                    .to_string()
            }
            Self::InstanceScriptItem { construct, .. } => format!(
                "the instance-script `{construct}` item (the supported instance script \
                 is a strict finite allowlist: a `let name = $state(<primitive>)` \
                 declarator, a single no-default `$props()` destructure, and a bare \
                 `let el;` used solely as a `bind:this` target)"
            ),
            Self::MagicIdentifier { name, .. } => format!(
                "the compiler-magic identifier `{name}` (the official compiler \
                 synthesizes it from the component signature; emitting a raw reference \
                 would bind an undefined identifier)"
            ),
            Self::ParagraphAutoclose { child, .. } => format!(
                "a `<p>` implicitly auto-closed by a `<{child}>` block child (the \
                 official compiler auto-closes the `<p>` and re-parents the block as a \
                 sibling; modeling that DOM re-parenting is outside the §1.2 core)"
            ),
            Self::ConstFoldThrow { reason, .. } => format!(
                "a constant-foldable interpolation whose evaluation throws at compile \
                 time ({reason}); the official compiler compile-fails the component, so \
                 Verter refuses rather than emitting live code that would crash at runtime"
            ),
            Self::ServerGenerate { .. } => {
                "server-side rendering (`generate: 'server'`)".to_string()
            }
        };
        format!(
            "Svelte client emission does not yet support {detail} (owning vertical {}).",
            self.owning_block()
        )
    }

    /// The source span the diagnostic refers to.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::DynamicAttribute { span, .. }
            | Self::SpreadOrHtml { span }
            | Self::Binding { span, .. }
            | Self::NonDelegatedEvent { span, .. }
            | Self::Block { span, .. }
            | Self::ComponentOrSnippet { span, .. }
            | Self::AdvancedRune { span, .. }
            | Self::HostOrCustomElement { span, .. }
            | Self::Element { span, .. }
            | Self::ElementName { span, .. }
            | Self::LegacyMode { span }
            | Self::ExperimentalAsync { span, .. }
            | Self::DevMode { span }
            | Self::Style { span }
            | Self::OptionsAxis { span }
            | Self::StaticInterpolation { span }
            | Self::DestructuringWrite { span }
            | Self::RootTextRegion { span }
            | Self::ComplexInterpolation { span }
            | Self::ScriptImport { span, .. }
            | Self::TypeScript { span }
            | Self::ComplexTextChunk { span }
            | Self::InstanceScriptItem { span, .. }
            | Self::MagicIdentifier { span, .. }
            | Self::ParagraphAutoclose { span, .. }
            | Self::ConstFoldThrow { span, .. }
            | Self::ServerGenerate { span } => *span,
        }
    }
}
