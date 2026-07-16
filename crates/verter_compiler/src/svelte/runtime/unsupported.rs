//! The closed family of unsupported Svelte runtime surfaces + its diagnostic
//! projection (the machine-stable diagnostic id, message, and span).

use super::official_rule::OfficialRejection;
use super::SvelteNamespace;
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
    /// A `<slot let:x>` producer-side provider `let:` binding. Official
    /// svelte@5.56.3 ACCEPTS the syntax but ITSELF emits broken output — a
    /// component-instance-scope `const x = $.derived_safe_equal(() =>
    /// $$slotProps.x);` reading an UNDECLARED `$$slotProps` (it is bound only
    /// inside a component slot-content callback), a guaranteed runtime
    /// `ReferenceError`. Verter refuses rather than shipping invalid runtime
    /// code — an accepted fail-closed upstream-bug divergence (the same class
    /// as the bare-`$host()` disposition), NOT a completeness deferral: there
    /// is no valid official topology to converge on while the pinned compiler
    /// emits the unbound read.
    SlotLetUnbound {
        /// The authored `let:` directive span (directive-precise, not the
        /// enclosing slot element).
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
    /// A `<style>` css body the scoping analyzer cannot PARSE or PROVE — a
    /// span-bearing CSS body-parse failure past the official-reject body
    /// probe, or an official `css_*` validation reject (`:global` / nesting
    /// placement), or a fail-closed render refusal (`css_render_failed`).
    /// Refused rather than emitted unscoped.
    StyleCssAnalysis {
        /// The PRECISE diagnostic code threaded from the typed style-plan
        /// failure (`css_expected_identifier` /
        /// `css_global_invalid_placement` / … / `css_render_failed`) —
        /// carried unchanged, never replaced by a generic surface id.
        code: &'static str,
        /// The source span of the offending css construct.
        span: Span,
    },
    /// A CLEAN-analyzed `<style>` whose selector⇄template relation the
    /// selector-to-template matcher cannot PROVE (a template construct outside
    /// the matcher's provable set — hoisted `<svelte:fragment slot>` content, a
    /// `<svelte:head>` `<title>`): without proven scope facts no faithful
    /// scoped emission exists, so the style refuses instead of emitting a
    /// guessed scope or unscoped output.
    StyleSelectorUnsupported {
        /// The selector-refusal diagnostic code threaded from the typed
        /// style-plan failure (`svelte-runtime-unsupported-style-selector`)
        /// — carried unchanged.
        code: &'static str,
        /// The source span of the UNPROVABLE construct itself.
        span: Span,
        /// The matcher's stable description of the unprovable construct
        /// class, when the refusal named one.
        construct: Option<&'static str>,
    },
    /// A css OUTPUT MODE the parse-domain detection cannot PROVE (a broken
    /// upstream invariant — the official rule is `inject_styles = css ===
    /// 'injected' || is_custom_element`, and both provable modes emit): an
    /// unprovable mode refuses rather than guessing a routing.
    StyleCssModeUnsupported {
        /// The source span (the `<style>` content).
        span: Span,
    },
    /// An officially-ACCEPTED compile option (or its inline `<svelte:options>`
    /// form) whose feature this backend deliberately does NOT support — a
    /// fail-closed FEATURE refusal that emits NO runtime module (NOT an official
    /// compile-error: the official `svelte@5.56.3` compiler accepts these). The
    /// closed set is `compatibility.componentApi: 4` / `hmr` / `accessors` /
    /// `immutable`; EXPLICIT presence rejects (including a `false` / default-
    /// equivalent value, and a value later masked by an inline override).
    CompileOptionUnsupported {
        /// The unsupported option.
        option: UnsupportedSvelteCompileOption,
        /// Where the option was set — the compile profile or the inline
        /// `<svelte:options>` element.
        origin: CompileOptionOrigin,
        /// The source span of the inline attribute carrier (the `<svelte:options>`
        /// open tag); `None` for a compile-profile option (no source location).
        span: Option<Span>,
    },
    /// A `namespace: 'svg' | 'mathml'` root selection — either the `namespace`
    /// compile option or an inline `<svelte:options namespace="svg">`. This backend
    /// supports the default `html` namespace ONLY; svg / mathml root element
    /// EMISSION (the `$.from_svg` / `$.from_mathml` root-helper layer, namespaced
    /// element cloning, recursive namespace inference) is a separate element-emission
    /// surface this backend does not yet emit, so a non-`html` namespace fails closed
    /// here rather than compiling a component whose namespaced elements would fail
    /// closed one-by-one downstream. A REFUSAL, not an official compile-error (the
    /// official `svelte@5.56.3` compiler accepts the option).
    NamespaceUnsupported {
        /// The requested non-`html` namespace (`svg` / `mathml`).
        namespace: SvelteNamespace,
        /// Where the namespace was set — the compile profile or the inline
        /// `<svelte:options>` element.
        origin: CompileOptionOrigin,
        /// The source span of the inline `<svelte:options>` open tag; `None` for a
        /// compile-profile option (no source location).
        span: Option<Span>,
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
    /// side-effect / mixed, instance or module slot — is admitted and
    /// hoisted through the typed [`UserImport`](super::client_imports::UserImport)
    /// prelude carrier, so this surface does not cover them.
    ScriptImport {
        /// A short construct label (`type-only import`, `import phase`,
        /// `import assertion`).
        construct: &'static str,
        /// The source span.
        span: Span,
    },
    /// A module-scope statement that requires dedicated Svelte lowering and cannot
    /// be emitted as an ordinary canonical statement. This includes module runes
    /// and TypeScript value constructs rejected by the pinned Svelte compiler
    /// (`enum`, value namespace, import-equals). Ordinary declarations, control
    /// flow, imports, and exports are admitted.
    ModuleScriptItem {
        /// A short construct label (`variable declaration`, `export`, `function`,
        /// `expression statement`, …).
        construct: &'static str,
        /// The source span (module-script-relative).
        span: Span,
    },
    /// A defensive residual for an unsupported TypeScript script form. Exact
    /// `lang="ts"` scripts use the canonical parser-wide erasure path; other
    /// language spellings remain JavaScript grammar and report parse diagnostics.
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
    /// A top-level instance-script construct that cannot use the canonical general
    /// statement carrier and has no dedicated Svelte lowering. Ordinary JavaScript
    /// declarations, expressions, and control flow are admitted. This residual
    /// covers unsupported TypeScript value constructs and Svelte-owned declaration
    /// shapes that must fail closed rather than leak source syntax.
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
    /// is not currently synthesized, so a magic-identifier reference fails
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
    /// store classifier rejects it rather than subscribing over the shadowed
    /// top-level base.
    StoreScopedSubscription {
        /// The `$`-prefixed accessor name whose base is scope-shadowed (`$x`).
        name: String,
        /// The source span of the offending reference (zero for a
        /// template-expression reference, whose arena entry carries no span).
        span: Span,
    },
    /// A template-expression FACT-RECOVERY failure: a scope-aware analysis
    /// (the `has_call` impure-call scan, the binding-impurity scan) could not
    /// re-derive its facts from an expression that lowered cleanly, or a
    /// canonical-analysis fact (the sync member/assignment wrap trigger) was
    /// demanded from a TORN expression. FAIL-CLOSED by contract: emitting with
    /// defaulted facts would silently change wrap / effect-membership /
    /// getter-vs-init topology (the official metadata would disagree), so the
    /// surface refuses with the failed analysis named instead of degrading.
    ExpressionFactRecovery {
        /// The analysis whose fact recovery failed (`has-call` /
        /// `binding-impurity` / `sync-member-or-assignment`).
        analysis: &'static str,
        /// The source span (`0..0` when the failing analysis carries no span).
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

/// The closed set of officially-ACCEPTED compile options this backend deliberately
/// does NOT support (a fail-closed FEATURE refusal, NOT an official compile-error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedSvelteCompileOption {
    /// `compatibility.componentApi` — the Svelte-4 component-instance API shape.
    /// Any explicit value other than `5` fails closed (`5` is the supported default).
    CompatibilityComponentApi,
    /// `hmr` — hot-module-replacement codegen.
    Hmr,
    /// `accessors` — the deprecated per-prop getter/setter accessors.
    Accessors,
    /// `immutable` — the deprecated immutable-data optimization hint.
    Immutable,
}

/// Where an [`UnsupportedSvelteCompileOption`] was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileOptionOrigin {
    /// A compile-profile option (the `SvelteRuntimeOptions` compile-option side).
    CompileProfile,
    /// An inline `<svelte:options>` attribute.
    Inline,
}

impl UnsupportedSvelteCompileOption {
    /// The distinct machine-stable diagnostic id fragment for this option.
    #[must_use]
    fn code(self) -> &'static str {
        match self {
            Self::CompatibilityComponentApi => {
                "svelte-runtime-unsupported-compatibility-component-api"
            }
            Self::Hmr => "svelte-runtime-unsupported-hmr",
            Self::Accessors => "svelte-runtime-unsupported-accessors",
            Self::Immutable => "svelte-runtime-unsupported-immutable",
        }
    }

    /// A human-readable label naming the option.
    #[must_use]
    fn label(self) -> &'static str {
        match self {
            Self::CompatibilityComponentApi => {
                "the `compatibility.componentApi` option (any explicit value other than `5`)"
            }
            Self::Hmr => "the `hmr` option",
            Self::Accessors => "the `accessors` option",
            Self::Immutable => "the `immutable` option",
        }
    }
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
            Self::SlotLetUnbound { .. } => "svelte-runtime-unsupported-slot-let-unbound",
            Self::AdvancedRune { .. } => "svelte-runtime-unsupported-advanced-rune",
            Self::HostOrCustomElement { .. } => "svelte-runtime-unsupported-host-custom-element",
            Self::Element { .. } => "svelte-runtime-unsupported-element",
            Self::ElementName { .. } => "svelte-runtime-unsupported-element-name",
            Self::ComponentExportBinding { .. } => {
                "svelte-runtime-unsupported-component-export-binding"
            }
            Self::LegacyRuneReference { .. } => "svelte-runtime-unsupported-legacy-rune-reference",
            Self::ExperimentalAsync { .. } => "svelte-runtime-unsupported-experimental-async",
            Self::DevMode { .. } => "svelte-runtime-unsupported-dev-mode",
            // The two style PLAN-FAILURE surfaces surface the CARRIED code —
            // the precise official css code (`css_global_invalid_placement` /
            // … / `css_render_failed`) or the fixed selector-refusal id —
            // threaded unchanged from the typed style-plan failure.
            Self::StyleCssAnalysis { code, .. } => code,
            Self::StyleSelectorUnsupported { code, .. } => code,
            Self::StyleCssModeUnsupported { .. } => "svelte-runtime-unsupported-style-css-mode",
            Self::CompileOptionUnsupported { option, .. } => option.code(),
            Self::NamespaceUnsupported { .. } => "svelte-runtime-unsupported-namespace",
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
            Self::ExpressionFactRecovery { .. } => {
                "svelte-runtime-unsupported-expression-fact-recovery"
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
            Self::SlotLetUnbound { .. } => "the `<slot let:…>` provider binding (the pinned \
                 official compiler emits an UNBOUND `$$slotProps` reference for it — a \
                 runtime `ReferenceError` — so Verter refuses rather than shipping \
                 invalid runtime code)"
                .to_string(),
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
            Self::LegacyRuneReference { rune, .. } => format!(
                "the `{rune}` rune name referenced in a legacy (non-runes) component (under \
                 `runes={{false}}` a rune name is a store subscription, not a rune)"
            ),
            Self::ExperimentalAsync { surface, .. } => {
                format!("the experimental-async `{surface}` surface")
            }
            Self::DevMode { .. } => "dev-mode (`dev: true`) codegen".to_string(),
            Self::StyleCssAnalysis { code, .. } => {
                format!(
                    "a `<style>` css construct the scoping analysis cannot parse or \
                     prove (a css body-parse failure, a `:global` / nesting placement \
                     violation, or a render refusal; `{code}`)"
                )
            }
            Self::StyleSelectorUnsupported { construct, .. } => {
                let named = construct.map(|c| format!(" ({c})")).unwrap_or_default();
                format!(
                    "a `<style>` selector/template relation the selector-to-template \
                     matcher cannot prove{named}; the style is refused rather than \
                     emitted with a guessed scope"
                )
            }
            Self::StyleCssModeUnsupported { .. } => {
                "a css output mode the parse-domain detection cannot prove (neither \
                 the external default nor `css=\"injected\"`/custom-element injection)"
                    .to_string()
            }
            Self::CompileOptionUnsupported { option, origin, .. } => {
                let origin = match origin {
                    CompileOptionOrigin::CompileProfile => "compile option",
                    CompileOptionOrigin::Inline => "`<svelte:options>` attribute",
                };
                format!("{} (set as a {origin})", option.label())
            }
            Self::NamespaceUnsupported {
                namespace, origin, ..
            } => {
                let ns = match namespace {
                    SvelteNamespace::Svg => "svg",
                    SvelteNamespace::Mathml => "mathml",
                    SvelteNamespace::Html => "html",
                };
                let origin = match origin {
                    CompileOptionOrigin::CompileProfile => "compile option",
                    CompileOptionOrigin::Inline => "`<svelte:options>` attribute",
                };
                format!(
                    "the `{ns}` namespace (set as a {origin}) — svg / mathml root element \
                     emission is a separate deferred surface; a non-`html` namespace is refused \
                     rather than emitted"
                )
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
                "the `<script module>` `{construct}` item (it requires dedicated \
                 Svelte lowering and cannot be emitted as an ordinary statement)"
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
                "the instance-script `{construct}` item (it has no safe canonical or \
                 dedicated Svelte lowering)"
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
            Self::ExpressionFactRecovery { analysis, .. } => {
                return format!(
                    "Svelte client emission could not recover the `{analysis}` \
                     analysis facts of a template expression; the surface fails \
                     closed rather than emitting with defaulted metadata."
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
            | Self::SlotLetUnbound { span }
            | Self::AdvancedRune { span, .. }
            | Self::HostOrCustomElement { span, .. }
            | Self::Element { span, .. }
            | Self::ElementName { span, .. }
            | Self::ComponentExportBinding { span, .. }
            | Self::LegacyRuneReference { span, .. }
            | Self::ExperimentalAsync { span, .. }
            | Self::DevMode { span }
            | Self::StyleCssAnalysis { span, .. }
            | Self::StyleSelectorUnsupported { span, .. }
            | Self::StyleCssModeUnsupported { span }
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
            | Self::ExpressionFactRecovery { span, .. }
            | Self::OfficialReject { span, .. } => *span,
            // A compile-profile option / namespace has no source span; an inline one
            // carries the `<svelte:options>` open-tag span.
            Self::CompileOptionUnsupported { span, .. }
            | Self::NamespaceUnsupported { span, .. } => span.unwrap_or_else(|| Span::new(0, 0)),
        }
    }

    /// The FAIL-CLOSED fact-recovery refusal for one named analysis — the
    /// shared constructor every fact-recovery `Err(())` maps through (no span
    /// is available at the analysis layer, so the diagnostic carries `0..0`).
    #[must_use]
    pub(super) fn expression_fact_recovery(analysis: &'static str) -> Self {
        Self::ExpressionFactRecovery {
            analysis,
            span: Span::new(0, 0),
        }
    }
}
