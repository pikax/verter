//! The closed family of unsupported Svelte runtime surfaces + its diagnostic
//! projection (the machine-stable diagnostic id, message, and span).

use super::official_rule::OfficialRejection;
use verter_span::Span;

/// The closed family of Svelte runtime surfaces this backend does NOT yet emit.
///
/// Each variant names ONE surface family; the fail-closed diagnostics are
/// identified by the machine-stable diagnostic id (`svelte-runtime-unsupported-<surface>`).
/// A surface that becomes supported deletes its own arm + tests cleanly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedSvelteRuntimeSurface {
    /// A dynamic attribute, boolean DOM prop, `class:` / `class={}`, `style:` /
    /// `style={}`, or a non-static property write not covered by the input
    /// cleanup.
    DynamicAttribute {
        /// The attribute / directive name.
        name: String,
        /// The source span.
        span: Span,
    },
    /// A binding beyond `bind:value` on an `<input>` and element `bind:this`.
    Binding {
        /// The bind target (`checked`, `group`, `this` on a component, …).
        target: String,
        /// The source span.
        span: Span,
    },
    /// A non-delegated / capture / global-target event, or a legacy `on:`
    /// modifier wrapper.
    NonDelegatedEvent {
        /// The event type.
        event_type: String,
        /// The source span.
        span: Span,
    },
    /// A control-flow block (`{#if}` / `{#each}` / `{#await}` / `{#key}`) or a
    /// declaration / `{@const}` / `{@debug}` tag.
    Block {
        /// A short construct label (`if`, `each`, `const`, …).
        construct: &'static str,
        /// The source span.
        span: Span,
    },
    /// A component, snippet, `{@render}`, `{@attach}`, transition, action,
    /// animation, or a renderable `<svelte:*>` element.
    ComponentOrSnippet {
        /// A short construct label.
        construct: &'static str,
        /// The source span.
        span: Span,
    },
    /// A rune beyond the supported `$state` / `$derived` / `$effect` / basic
    /// `$props()` subset (`$state.raw` / `$state.snapshot` / `$effect.pre` /
    /// `$effect.root` / `$effect.tracking` / `$props()` rest / `$props.id` /
    /// `$bindable`), or a `$inspect` reference OUTSIDE the production-elided
    /// statement positions (the statement forms are supported as elision; a
    /// non-statement-position `$inspect` reference fails closed at the rewriter).
    AdvancedRune {
        /// A short rune label.
        rune: &'static str,
        /// The source span.
        span: Span,
    },
    /// A `$host()` rune (a custom-element-only API) / custom-element output.
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
    /// change. This is the "regular element not yet in the client core" refusal,
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
    /// identifier-unsafe element tag fails closed rather than emitting invalid JS.
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
    /// reactive-text op.
    StaticInterpolation {
        /// The source span of the interpolation expression.
        span: Span,
    },
    /// A DESTRUCTURING assignment / update target outside the supported lvalue
    /// subset (`({ count } = obj)` / `[count] = arr`). The official compiler lowers
    /// it through a destructure closure (`$.set(count, obj.count, true)`); that
    /// lowering is a deferral-ledger follow-up, so a destructuring write target
    /// fails closed rather than emitting a raw (un-rewritten) destructuring
    /// assignment to a reactive binding.
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
    /// undeclared text-node var (invalid JS).
    RootTextRegion {
        /// The source span of the root interpolation region.
        span: Span,
    },
    /// An instance-script `export const` / `export function` / `export class`
    /// component-export binding (ANY mode). Official lowers these through the
    /// `$$exports` accessor object (`var $$exports = { … }` + `$.bind_prop($$props,
    /// key, value)` + `return $.pop($$exports)`); that accessor mechanism is not
    /// yet emitted, so the surface fails closed under its OWN identity — never the
    /// prop surface, never the generic export residual.
    ComponentExportBinding {
        /// The exported declaration keyword (`const` / `function` / `class`).
        construct: &'static str,
        /// The source span of the export declaration.
        span: Span,
    },
    /// A `<slot>` element in a LEGACY (non-runes) component — the legacy slot
    /// surface (`$.slot(node, $$props, 'default', …)`). Not yet lowered — fails
    /// closed.
    LegacySlotElement {
        /// The source span of the slot element.
        span: Span,
    },
    /// A `createEventDispatcher` usage (the `svelte` import local referenced in
    /// the instance script) in a LEGACY (non-runes) component — the legacy
    /// component-event surface. Not yet lowered — fails closed.
    LegacyEventDispatcher {
        /// The source span of the dispatcher reference.
        span: Span,
    },
    /// A Svelte rune NAME referenced in a LEGACY (non-runes) component — under
    /// `runes={false}` a rune name is NOT a rune (official parses `$state` as a
    /// STORE subscription of `state` and lowers `let` state through
    /// `$.mutable_source`), a semantic this backend does not emit. Fails closed
    /// rather than mis-lowering the reference as a rune.
    LegacyRuneReference {
        /// The referenced rune root name (`$state` / `$derived` / …).
        rune: String,
        /// The source span.
        span: Span,
    },
    /// Experimental async (`$state.eager` / `$effect.pending` / async block
    /// helpers / async `$derived`).
    ExperimentalAsync {
        /// A short rune / surface label.
        surface: &'static str,
        /// The source span.
        span: Span,
    },
    /// A dev-mode (`dev: true`) codegen request — the dev-mode output axis
    /// (validation wrappers, `$.add_locations`, dev `$inspect` / `$.trace`) is not
    /// emitted; only the production runes client output is.
    DevMode {
        /// The source span.
        span: Span,
    },
    /// A top-level `<style>` / CSS scoping surface.
    Style {
        /// The source span.
        span: Span,
    },
    /// `<svelte:options>` / a compile-option axis beyond name/runes/client.
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
    /// so it fails closed.
    ComplexInterpolation {
        /// The source span of the interpolation expression.
        span: Span,
    },
    /// A residual NON-static import form the static-import prelude does not admit: a
    /// TypeScript type-only import in a plain script (`import type …` /
    /// `import { type T }` — an official parse error outside `lang="ts"`), an import
    /// PHASE (`import defer …` / source-phase), or the deprecated `assert { … }`
    /// attribute keyword (official parse-rejects it; only `with { … }` is preserved).
    /// Every STATIC import form — default / named / aliased / namespace /
    /// side-effect / mixed, instance or import-only module slot — is admitted and
    /// hoisted through the typed [`UserImport`](super::client_imports::UserImport)
    /// prelude carrier, so this surface no longer covers them.
    ScriptImport {
        /// A short construct label (`type-only import`, `import phase`,
        /// `import assertion`).
        construct: &'static str,
        /// The source span.
        span: Span,
    },
    /// A NON-import top-level statement in a `<script module>`. The admitted module
    /// script is IMPORT-ONLY: every top-level statement must be a static `import`
    /// declaration. Arbitrary module items — a variable / function / class
    /// declaration, an export or re-export (`export … from`), an expression
    /// statement, control flow, or a module-scope rune (`$state` / `$derived` /
    /// `$props.id()`) — are the module-item completion surface and fail closed here
    /// with the offending statement family.
    ModuleScriptItem {
        /// A short construct label (`variable declaration`, `export`, `function`,
        /// `expression statement`, …).
        construct: &'static str,
        /// The source span (module-script-relative).
        span: Span,
    },
    /// A `<script lang="ts">` / `<script lang="tsx">` (TypeScript) script. The
    /// official compiler strips the TS annotations before lowering; the TS-strip
    /// path is a script-completion follow-up, so a TypeScript script fails closed
    ///.
    TypeScript {
        /// The source span.
        span: Span,
    },
    /// A reactive-text run whose LITERAL chunk is not simple ASCII (an HTML entity
    /// reference, a tab / newline / repeated-space run, or a character needing
    /// template-literal escaping / whitespace normalization). The supported literal
    /// chunk is simple ASCII; a complex chunk needs the official boundary-trimming /
    /// entity-decode / escaping path — a reactive-text-completion follow-up, so it
    /// fails closed.
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
    /// statement, a non-`$` labeled statement, or a `$` / `$$`-prefixed binding —
    /// fails closed here BY CONSTRUCTION rather than reaching the broad
    /// statement-rewrite lowering. (A legacy-mode `$:` reactive statement is a
    /// SUPPORTED shape and lowers; the runes-mode `$:` twin is the official
    /// `legacy_reactive_statement_invalid` reject — [`Self::OfficialReject`], not
    /// this variant.) Adding a shape requires extending the finite
    /// [`SupportedInstanceScriptItem`](super::client_shapes::SupportedInstanceScriptItem)
    /// enum AND adding a golden in the same change. The broader instance-script
    /// surface (functions, statements, multi-declarators) is not yet supported.
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
    /// is a script-completion follow-up, so a magic-identifier reference fails
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
    /// outside the §1.2 core, so it fails closed as an unsupported FEATURE rather
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
    /// still refuses, because official evaluates both before selecting. Cases:
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
    /// SSR (`generate: 'server'`) — the server backend is not yet implemented.
    ServerGenerate {
        /// The source span.
        span: Span,
    },
    /// A `$NAME` store subscription whose BASE resolves — in the reference's
    /// REAL lexical scope — to a NON-top-level binding: a `{#each as x}` alias,
    /// a `{#snippet}` parameter, an `{#await then x}` binding, a slot `let:`
    /// local, or a script/expression function parameter shadowing the
    /// top-level store base. Official `svelte@5.56.3` COMPILE-ERRORS this class
    /// (`store_invalid_scoped_subscription` — "Cannot subscribe to stores that
    /// are not declared at the top level of the component"); the scope-aware
    /// store classifier rejects it instead of subscribing over the shadowed
    /// top-level base (the former fail-open).
    StoreScopedSubscription {
        /// The `$`-prefixed accessor name whose base is scope-shadowed (`$x`).
        name: String,
        /// The source span of the offending reference (zero for a
        /// template-expression reference, whose arena entry carries no span).
        span: Span,
    },
    /// A carrier for an OFFICIAL-reject detected MID-REWRITE. The fallible
    /// expression rewriter's error channel is `UnsupportedSvelteRuntimeSurface`,
    /// but some malformed inputs it detects (a `$$`-prefixed member of a
    /// `$props()` rest / whole-object binding — official `props_illegal_name`) are
    /// official COMPILE-ERRORS, not unsupported features. This variant carries the
    /// typed [`OfficialRejection`] out of the rewriter; the
    /// `From<UnsupportedSvelteRuntimeSurface> for ClientCompileError` conversion at
    /// the compile boundary maps it to [`ClientCompileError::OfficialReject`], so it
    /// NEVER surfaces as an `Unsupported` diagnostic. Its `diagnostic_code` /
    /// `message` still delegate to the carried rule so any defensive surfacing stays
    /// correct.
    ///
    /// [`ClientCompileError::OfficialReject`]: super::ClientCompileError::OfficialReject
    OfficialReject {
        /// The official-reject rule class + the exact official diagnostic code.
        rejection: OfficialRejection,
        /// The source span.
        span: Span,
    },
}

impl UnsupportedSvelteRuntimeSurface {
    /// The machine-stable diagnostic id (`svelte-runtime-unsupported-<surface>`).
    #[must_use]
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::DynamicAttribute { .. } => "svelte-runtime-unsupported-dynamic-attribute",
            Self::Binding { .. } => "svelte-runtime-unsupported-binding",
            Self::NonDelegatedEvent { .. } => "svelte-runtime-unsupported-non-delegated-event",
            Self::Block { .. } => "svelte-runtime-unsupported-block",
            Self::ComponentOrSnippet { .. } => "svelte-runtime-unsupported-component",
            Self::AdvancedRune { .. } => "svelte-runtime-unsupported-advanced-rune",
            Self::HostOrCustomElement { .. } => "svelte-runtime-unsupported-host-custom-element",
            Self::Element { .. } => "svelte-runtime-unsupported-element",
            Self::ElementName { .. } => "svelte-runtime-unsupported-element-name",
            Self::ComponentExportBinding { .. } => {
                "svelte-runtime-unsupported-component-export-binding"
            }
            Self::LegacySlotElement { .. } => "svelte-runtime-unsupported-legacy-slot",
            Self::LegacyEventDispatcher { .. } => {
                "svelte-runtime-unsupported-legacy-event-dispatcher"
            }
            Self::LegacyRuneReference { .. } => "svelte-runtime-unsupported-legacy-rune-reference",
            Self::ExperimentalAsync { .. } => "svelte-runtime-unsupported-experimental-async",
            Self::DevMode { .. } => "svelte-runtime-unsupported-dev-mode",
            Self::Style { .. } => "svelte-runtime-unsupported-style",
            Self::OptionsAxis { .. } => "svelte-runtime-unsupported-options",
            Self::StaticInterpolation { .. } => "svelte-runtime-unsupported-static-interpolation",
            Self::DestructuringWrite { .. } => "svelte-runtime-unsupported-destructuring-write",
            Self::RootTextRegion { .. } => "svelte-runtime-unsupported-root-text-region",
            Self::ComplexInterpolation { .. } => "svelte-runtime-unsupported-complex-interpolation",
            Self::ScriptImport { .. } => "svelte-runtime-unsupported-script-import",
            Self::ModuleScriptItem { .. } => "svelte-runtime-unsupported-module-script-item",
            Self::TypeScript { .. } => "svelte-runtime-unsupported-typescript",
            Self::ComplexTextChunk { .. } => "svelte-runtime-unsupported-complex-text",
            Self::InstanceScriptItem { .. } => "svelte-runtime-unsupported-instance-script-item",
            Self::MagicIdentifier { .. } => "svelte-runtime-unsupported-magic-identifier",
            Self::ParagraphAutoclose { .. } => "svelte-runtime-unsupported-paragraph-autoclose",
            Self::ConstFoldThrow { .. } => "svelte-runtime-unsupported-const-fold-throw",
            Self::ServerGenerate { .. } => "svelte-runtime-unsupported-server-generate",
            Self::StoreScopedSubscription { .. } => {
                "svelte-runtime-unsupported-store-scoped-subscription"
            }
            // A transient official-reject carrier — delegate to the carried rule's
            // official-reject diagnostic id (it is converted to
            // `ClientCompileError::OfficialReject` at the boundary and never surfaces
            // through the unsupported family).
            Self::OfficialReject { rejection, .. } => rejection.rule.diagnostic_code(),
        }
    }

    /// A human-readable message naming the unsupported surface.
    #[must_use]
    pub fn message(&self) -> String {
        let detail = match self {
            Self::DynamicAttribute { name, .. } => {
                format!("the dynamic attribute / directive `{name}`")
            }
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
            Self::ComponentExportBinding { construct, .. } => format!(
                "an instance-script `export {construct}` component-export binding (official \
                 lowers it through the `$$exports` accessor object + `$.bind_prop`)"
            ),
            Self::LegacySlotElement { .. } => {
                "a `<slot>` element in a legacy (non-runes) component".to_string()
            }
            Self::LegacyEventDispatcher { .. } => {
                "a `createEventDispatcher` usage in a legacy (non-runes) component".to_string()
            }
            Self::LegacyRuneReference { rune, .. } => format!(
                "the `{rune}` rune name referenced in a legacy (non-runes) component (under \
                 `runes={{false}}` a rune name is a store subscription, not a rune)"
            ),
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
                format!(
                    "a `{construct}` (only static value imports are admitted to the \
                     module import prelude)"
                )
            }
            Self::ModuleScriptItem { construct, .. } => format!(
                "the `<script module>` `{construct}` item (the admitted module script \
                 is import-only; other module items are not yet lowered)"
            ),
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
            // Mirrors the official `store_invalid_scoped_subscription` COMPILE-ERROR
            // — surface the official-shaped message directly (NOT the "does not yet
            // support …" wrapper below): the input is rejected by the official
            // compiler, not a deferrable Verter feature.
            Self::StoreScopedSubscription { name, .. } => {
                return format!(
                    "cannot subscribe to stores that are not declared at the top level \
                     of the component (`{name}` resolves to a template-block / \
                     function-local binding; official `store_invalid_scoped_subscription`)"
                );
            }
            // A transient official-reject carrier — surface the carried rule's
            // official-reject message directly (NOT the "does not yet support …"
            // wrapper below), since it mirrors an official COMPILE-ERROR, not a
            // deferrable feature.
            Self::OfficialReject { rejection, .. } => return rejection.rule.message(),
        };
        format!("Svelte client emission does not yet support {detail}.")
    }

    /// The source span the diagnostic refers to.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::DynamicAttribute { span, .. }
            | Self::Binding { span, .. }
            | Self::NonDelegatedEvent { span, .. }
            | Self::Block { span, .. }
            | Self::ComponentOrSnippet { span, .. }
            | Self::AdvancedRune { span, .. }
            | Self::HostOrCustomElement { span, .. }
            | Self::Element { span, .. }
            | Self::ElementName { span, .. }
            | Self::ComponentExportBinding { span, .. }
            | Self::LegacySlotElement { span }
            | Self::LegacyEventDispatcher { span }
            | Self::LegacyRuneReference { span, .. }
            | Self::ExperimentalAsync { span, .. }
            | Self::DevMode { span }
            | Self::Style { span }
            | Self::OptionsAxis { span }
            | Self::StaticInterpolation { span }
            | Self::DestructuringWrite { span }
            | Self::RootTextRegion { span }
            | Self::ComplexInterpolation { span }
            | Self::ScriptImport { span, .. }
            | Self::ModuleScriptItem { span, .. }
            | Self::TypeScript { span }
            | Self::ComplexTextChunk { span }
            | Self::InstanceScriptItem { span, .. }
            | Self::MagicIdentifier { span, .. }
            | Self::ParagraphAutoclose { span, .. }
            | Self::ConstFoldThrow { span, .. }
            | Self::ServerGenerate { span }
            | Self::StoreScopedSubscription { span, .. }
            | Self::OfficialReject { span, .. } => *span,
        }
    }
}
