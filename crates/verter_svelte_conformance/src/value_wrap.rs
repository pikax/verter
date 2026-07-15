//! The legacy VALUE-WRAP × SURFACE coverage axis: the typed COVERAGE oracle
//! for the official `build_expression` legacy wrap
//! (`(dep, …, $.untrack(() => value))`) over the enumerated
//! [`ValueWrapSurface`] vocabulary — the committed mirror of the compiler's
//! `AuthoredValueSurface`, freshness-pinned by the executable gate. This is
//! coverage over the INVENTORIED authored-value surfaces, NOT a completeness
//! proof over every authored byte the client backend accepts: the D-61
//! raw-string authored-emission residual
//! (`docs/arch/svelte-native-compiler-plan.md`) is the deferred capability
//! boundary and is NOT covered here.
//!
//! The axis is TYPED ROLES only — never fixture paths: [`ValueWrapSurface`]
//! is the closed surface vocabulary (a committed mirror of the compiler's
//! `AuthoredValueSurface`, freshness-pinned by the executable gate), and
//! [`classify_value_wrap`] is the ONE exhaustive, wildcard-free
//! classification of each surface into its [`LegacyWrapPolicy`] (whether the
//! official compiler routes the surface through `build_expression`) plus its
//! [`TriggerReachability`] (whether the wrap trigger —
//! `has_call || has_member || has_assignment` — is expressible on the surface
//! at all). Each cell renders an executable `.svelte` fixture per
//! [`WrapMode`]; the `value_wrap_cells` gate compiles every cell through
//! Verter's PRODUCTION client pipeline and asserts the wrap observation
//! (`$.untrack` presence) matches the classification — a surface added to the
//! compiler without a cell here fails the freshness pin (reject-unclassified),
//! and a cell whose classification contradicts the emission fails the
//! observation gate.

/// The closed authored-value surface vocabulary — the value-wrap coverage
/// axis. Mirrors the compiler's `AuthoredValueSurface` (variant-name parity is
/// pinned by `mirror_matches_compiler_surface_vocabulary`).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ValueWrapSurface {
    /// A reactive text interpolation.
    ReactiveText,
    /// A regular DOM attribute / property value.
    AttributeValue,
    /// An ordinary component prop value.
    ComponentProp,
    /// An ordinary `<slot>` prop value.
    SlotProp,
    /// A `{@render}` argument.
    RenderArg,
    /// The `{@render}` DYNAMIC-callee slice (the callee peeled from a dynamic
    /// render call).
    RenderCallee,
    /// The authored `class={expr}` single base (before `$.clsx`).
    ClassBase,
    /// The authored `style={expr}` single base.
    StyleBase,
    /// A `style:name={expr}` directive value.
    StyleDirectiveValue,
    /// A co-located ordinary attribute value inside an attribute effect.
    AttributeEffectValue,
    /// An `{#if}` / `{:else if}` test.
    IfCondition,
    /// The `{#each}` collection.
    EachCollection,
    /// The `{#await}` promise.
    AwaitPromise,
    /// The `{#key}` expression.
    KeyExpression,
    /// The `{@html}` payload.
    HtmlPayload,
    /// A `{@const}` initializer.
    ConstInitializer,
    /// An element-position `{@attach}` payload.
    AttachPayload,
    /// An authored `<title>` chunk inside `<svelte:head>`.
    TitleChunk,
    /// A component spread operand.
    ComponentSpreadOperand,
    /// A `<slot>` spread operand.
    SlotSpreadOperand,
    /// A regular-element / `<svelte:element>` spread operand.
    ElementSpreadOperand,
    /// A `class:name={expr}` directive condition.
    ClassDirectiveCondition,
    /// A `{const …}` / `{let …}` declaration-tag initializer.
    DeclarationTagInitializer,
    /// A `use:action={arg}` argument.
    UseActionArg,
    /// A `transition:`/`in:`/`out:` params argument.
    TransitionParams,
    /// An `animate:` params argument.
    AnimationParams,
    /// An ordinary event-handler expression.
    EventHandler,
    /// A keyed-each KEY expression.
    EachKeyExpression,
    /// The `<svelte:element this={expr}>` tag expression.
    SvelteElementThis,
    /// The `<svelte:component this={expr}>` selector.
    ComponentSelector,
    /// A `{@debug}` argument.
    DebugArg,
    /// A `<svelte:boundary>` `onerror` / `failed` / `pending` attribute prop.
    BoundaryProp,
}

impl ValueWrapSurface {
    /// Every surface, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::ReactiveText,
        Self::AttributeValue,
        Self::ComponentProp,
        Self::SlotProp,
        Self::RenderArg,
        Self::RenderCallee,
        Self::ClassBase,
        Self::StyleBase,
        Self::StyleDirectiveValue,
        Self::AttributeEffectValue,
        Self::IfCondition,
        Self::EachCollection,
        Self::AwaitPromise,
        Self::KeyExpression,
        Self::HtmlPayload,
        Self::ConstInitializer,
        Self::AttachPayload,
        Self::TitleChunk,
        Self::ComponentSpreadOperand,
        Self::SlotSpreadOperand,
        Self::ElementSpreadOperand,
        Self::ClassDirectiveCondition,
        Self::DeclarationTagInitializer,
        Self::UseActionArg,
        Self::TransitionParams,
        Self::AnimationParams,
        Self::EventHandler,
        Self::EachKeyExpression,
        Self::SvelteElementThis,
        Self::ComponentSelector,
        Self::DebugArg,
        Self::BoundaryProp,
    ];

    /// The compiler-enum variant name (the freshness-pin key).
    #[must_use]
    pub fn variant_name(self) -> &'static str {
        match self {
            Self::ReactiveText => "ReactiveText",
            Self::AttributeValue => "AttributeValue",
            Self::ComponentProp => "ComponentProp",
            Self::SlotProp => "SlotProp",
            Self::RenderArg => "RenderArg",
            Self::RenderCallee => "RenderCallee",
            Self::ClassBase => "ClassBase",
            Self::StyleBase => "StyleBase",
            Self::StyleDirectiveValue => "StyleDirectiveValue",
            Self::AttributeEffectValue => "AttributeEffectValue",
            Self::IfCondition => "IfCondition",
            Self::EachCollection => "EachCollection",
            Self::AwaitPromise => "AwaitPromise",
            Self::KeyExpression => "KeyExpression",
            Self::HtmlPayload => "HtmlPayload",
            Self::ConstInitializer => "ConstInitializer",
            Self::AttachPayload => "AttachPayload",
            Self::TitleChunk => "TitleChunk",
            Self::ComponentSpreadOperand => "ComponentSpreadOperand",
            Self::SlotSpreadOperand => "SlotSpreadOperand",
            Self::ElementSpreadOperand => "ElementSpreadOperand",
            Self::ClassDirectiveCondition => "ClassDirectiveCondition",
            Self::DeclarationTagInitializer => "DeclarationTagInitializer",
            Self::UseActionArg => "UseActionArg",
            Self::TransitionParams => "TransitionParams",
            Self::AnimationParams => "AnimationParams",
            Self::EventHandler => "EventHandler",
            Self::EachKeyExpression => "EachKeyExpression",
            Self::SvelteElementThis => "SvelteElementThis",
            Self::ComponentSelector => "ComponentSelector",
            Self::DebugArg => "DebugArg",
            Self::BoundaryProp => "BoundaryProp",
        }
    }

    /// The stable slug fragment of this surface's cells.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Self::ReactiveText => "text",
            Self::AttributeValue => "attr",
            Self::ComponentProp => "comp-prop",
            Self::SlotProp => "slot-prop",
            Self::RenderArg => "render-arg",
            Self::RenderCallee => "render-callee",
            Self::ClassBase => "class-base",
            Self::StyleBase => "style-base",
            Self::StyleDirectiveValue => "style-dir",
            Self::AttributeEffectValue => "fold-attr",
            Self::IfCondition => "if-test",
            Self::EachCollection => "each-src",
            Self::AwaitPromise => "await-promise",
            Self::KeyExpression => "key-expr",
            Self::HtmlPayload => "html",
            Self::ConstInitializer => "const-init",
            Self::AttachPayload => "attach",
            Self::TitleChunk => "title-chunk",
            Self::ComponentSpreadOperand => "comp-spread",
            Self::SlotSpreadOperand => "slot-spread",
            Self::ElementSpreadOperand => "el-spread",
            Self::ClassDirectiveCondition => "class-dir",
            Self::DeclarationTagInitializer => "decl-init",
            Self::UseActionArg => "use-arg",
            Self::TransitionParams => "transition-params",
            Self::AnimationParams => "animate-params",
            Self::EventHandler => "event-handler",
            Self::EachKeyExpression => "each-key",
            Self::SvelteElementThis => "element-this",
            Self::ComponentSelector => "component-this",
            Self::DebugArg => "debug-arg",
            Self::BoundaryProp => "boundary-prop",
        }
    }
}

/// Whether the official compiler routes the surface through
/// `build_expression` (the wrap applies in a definitely-legacy component when
/// the trigger fires) or visits it RAW.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyWrapPolicy {
    /// `build_expression` applies — the surface WRAPS in definite legacy.
    BuildExpression,
    /// The surface is visited raw — the wrap NEVER applies.
    Raw,
}

/// Whether the wrap TRIGGER (`has_call || has_member || has_assignment`) is
/// expressible on the surface's ACCEPTED grammar at all — the `{@debug}`
/// argument grammar admits identifiers only, and the accepted event-handler
/// shapes are function-valued (a call/member inside a function body is not a
/// synchronous trigger) — so those cells pin the unconditional raw emission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerReachability {
    /// A synchronous call/member payload is expressible — the cell
    /// discriminates the policy (flipping the classification flips the
    /// observation).
    Observable,
    /// The accepted surface grammar cannot carry a synchronous trigger, so
    /// the cell pins the raw emission without discriminating the policy axis.
    TriggerUnreachable,
}

/// The compile mode a cell fixture is compiled under.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WrapMode {
    /// Definitely legacy (`export let` marker): the wrap applies to
    /// `BuildExpression` surfaces.
    DefiniteLegacy,
    /// The official maybe-runes in-between mode (store-only component): the
    /// wrap NEVER applies.
    MaybeRunes,
}

/// One classified value-wrap × surface cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueWrapCell {
    /// The surface role.
    pub surface: ValueWrapSurface,
    /// The wrap classification (the official `build_expression` routing).
    pub policy: LegacyWrapPolicy,
    /// Whether the trigger is expressible on the surface.
    pub reachability: TriggerReachability,
}

/// The ONE exhaustive, wildcard-free value-wrap classification — the covering
/// cell of each surface. Grounded against the pinned official `svelte@5.56.3`
/// client transform (`shared/utils.js` `build_expression` and its direct +
/// `build_attribute_value` / `build_template_chunk` consumers).
#[must_use]
pub const fn classify_value_wrap(surface: ValueWrapSurface) -> ValueWrapCell {
    use LegacyWrapPolicy::{BuildExpression, Raw};
    use TriggerReachability::{Observable, TriggerUnreachable};
    use ValueWrapSurface as S;
    let (policy, reachability) = match surface {
        S::ReactiveText
        | S::AttributeValue
        | S::ComponentProp
        | S::SlotProp
        | S::RenderArg
        | S::RenderCallee
        | S::ClassBase
        | S::StyleBase
        | S::StyleDirectiveValue
        | S::AttributeEffectValue
        | S::IfCondition
        | S::EachCollection
        | S::AwaitPromise
        | S::KeyExpression
        | S::HtmlPayload
        | S::ConstInitializer
        | S::AttachPayload
        | S::TitleChunk => (BuildExpression, Observable),
        S::ComponentSpreadOperand
        | S::SlotSpreadOperand
        | S::ElementSpreadOperand
        | S::ClassDirectiveCondition
        | S::DeclarationTagInitializer
        | S::UseActionArg
        | S::TransitionParams
        | S::AnimationParams
        | S::EachKeyExpression
        | S::SvelteElementThis
        | S::ComponentSelector
        | S::BoundaryProp => (Raw, Observable),
        S::EventHandler | S::DebugArg => (Raw, TriggerUnreachable),
    };
    ValueWrapCell {
        surface,
        policy,
        reachability,
    }
}

/// Every classified cell, in surface declaration order — the covering
/// enumeration of the value-wrap × surface axis.
#[must_use]
pub fn value_wrap_cells() -> Vec<ValueWrapCell> {
    ValueWrapSurface::ALL
        .iter()
        .map(|&surface| classify_value_wrap(surface))
        .collect()
}

/// Render the executable `.svelte` fixture of one cell under `mode`. Every
/// fixture embeds the SAME call-bearing authored payload (`obj.m()` /
/// `$s.m()`) at the cell's surface — the mode prelude decides the component
/// mode (definite legacy via `export let`; maybe-runes via a store-only
/// script), and the surface template places the payload. Typed rendering
/// only — no path, no slug, no source scan decides a cell.
#[must_use]
pub fn render_cell_fixture(surface: ValueWrapSurface, mode: WrapMode) -> String {
    use ValueWrapSurface as S;
    // The mode prelude + the authored call payload spelling. Surfaces that
    // need extra script items (imports, snippet defs) add them per-arm.
    let (prelude, payload) = match mode {
        WrapMode::DefiniteLegacy => ("export let obj;", "obj.m()"),
        WrapMode::MaybeRunes => (
            "import { writable } from 'svelte/store';\nconst s = writable({});",
            "$s.m()",
        ),
    };
    let script = |extra: &str| {
        if extra.is_empty() {
            format!("<script>{prelude}</script>\n")
        } else {
            format!("<script>{prelude}\n{extra}</script>\n")
        }
    };
    match surface {
        // The reactive-text completion surface accepts the namespace-import
        // member shape (`{NS.z}`) — a general call/member interpolation is a
        // separate fail-closed completion surface — so the TEXT cell uses the
        // MEMBER trigger on the accepted shape (equally wrap-observable). The
        // maybe-runes marker is the ABSENCE of legacy markers (a store const
        // would be unadmitted without a `$s` template read).
        S::ReactiveText => match mode {
            WrapMode::DefiniteLegacy => format!(
                "{}<p>{{NS.z}}</p>\n",
                script("import * as NS from './x.js';")
            ),
            WrapMode::MaybeRunes => {
                "<script>import * as NS from './x.js';</script>\n<p>{NS.z}</p>\n".to_string()
            }
        },
        S::AttributeValue => format!("{}<div title={{{payload}}}></div>\n", script("")),
        S::ComponentProp => format!(
            "{}<Child foo={{{payload}}} />\n",
            script("import Child from './Child.svelte';")
        ),
        S::SlotProp => format!("{}<div><slot foo={{{payload}}} /></div>\n", script("")),
        S::RenderArg => format!(
            "{}{{#snippet block(x)}}<p>{{x}}</p>{{/snippet}}\n{{@render block({payload})}}\n",
            script("")
        ),
        // The DYNAMIC render callee is the wrap-eligible payload: `{@render
        // obj.m()(1)}` peels the trailing `(1)`, leaving the callee `obj.m()`
        // (a member-rooted call on a legacy prop) — the peeled RenderCalleeSlice
        // callee wraps, so the legacy emission carries the `$.untrack` wrap on
        // the callee.
        S::RenderCallee => format!("{}{{@render {payload}(1)}}\n", script("")),
        S::ClassBase => format!("{}<div class={{{payload}}}></div>\n", script("")),
        S::StyleBase => format!("{}<div style={{{payload}}}></div>\n", script("")),
        S::StyleDirectiveValue => format!("{}<div style:color={{{payload}}}></div>\n", script("")),
        // The spread source must not add a mode marker: the legacy cell spreads
        // a legacy prop; the maybe-runes cell spreads a store member.
        S::AttributeEffectValue => match mode {
            WrapMode::DefiniteLegacy => format!(
                "{}<div {{...p}} title={{{payload}}}></div>\n",
                script("export let p;")
            ),
            WrapMode::MaybeRunes => format!(
                "{}<div {{...$s.rest}} title={{{payload}}}></div>\n",
                script("")
            ),
        },
        S::IfCondition => format!("{}{{#if {payload}}}<p>a</p>{{/if}}\n", script("")),
        S::EachCollection => format!(
            "{}{{#each {payload} as item}}<p>{{item}}</p>{{/each}}\n",
            script("")
        ),
        S::AwaitPromise => format!(
            "{}{{#await {payload} then v}}<p>{{v}}</p>{{/await}}\n",
            script("")
        ),
        S::KeyExpression => format!("{}{{#key {payload}}}<p>a</p>{{/key}}\n", script("")),
        S::HtmlPayload => format!("{}{{@html {payload}}}\n", script("")),
        S::ConstInitializer => format!(
            "{}{{#each [1] as item}}{{@const y = {payload}}}<p>{{y}}</p>{{/each}}\n",
            script("")
        ),
        S::AttachPayload => format!("{}<div {{@attach {payload}}}></div>\n", script("")),
        S::TitleChunk => format!(
            "{}<svelte:head><title>{{{payload}}}</title></svelte:head>\n",
            script("")
        ),
        S::ComponentSpreadOperand => format!(
            "{}<Child {{...{payload}}} />\n",
            script("import Child from './Child.svelte';")
        ),
        S::SlotSpreadOperand => format!("{}<div><slot {{...{payload}}} /></div>\n", script("")),
        S::ElementSpreadOperand => format!("{}<div {{...{payload}}}></div>\n", script("")),
        S::ClassDirectiveCondition => format!("{}<div class:on={{{payload}}}></div>\n", script("")),
        S::DeclarationTagInitializer => format!(
            "{}{{#each [1] as item}}{{const y = {payload}}}x{{/each}}\n",
            script("")
        ),
        S::UseActionArg => format!(
            "{}<div use:act={{{payload}}}></div>\n",
            script("import { act } from './x.js';")
        ),
        S::TransitionParams => format!(
            "{}<div transition:fade={{{payload}}}></div>\n",
            script("import { fade } from 'svelte/transition';")
        ),
        S::AnimationParams => format!(
            "{}{{#each [1] as item (item)}}<p animate:flip={{{payload}}}>{{item}}</p>{{/each}}\n",
            script("import { flip } from 'svelte/animate';")
        ),
        // The accepted delegated handler shape is a nullary arrow of signal
        // writes — a write inside the function body is not a synchronous
        // trigger, so this cell pins the unconditional raw emission.
        S::EventHandler => match mode {
            WrapMode::DefiniteLegacy => format!(
                "{}<button onclick={{() => n += 1}}>x</button>\n",
                script("let n = 0;")
            ),
            WrapMode::MaybeRunes => format!(
                "{}<button onclick={{() => $s += 1}}>x</button>\n",
                script("")
            ),
        },
        S::EachKeyExpression => {
            // The KEY is the surface; the collection is a plain array so the
            // key expression is the only wrap-eligible payload position.
            format!(
                "{}{{#each [1] as item ({payload})}}<p>{{item}}</p>{{/each}}\n",
                script("")
            )
        }
        S::SvelteElementThis => format!(
            "{}<svelte:element this={{{payload}}} title=\"t\"></svelte:element>\n",
            script("")
        ),
        S::ComponentSelector => format!("{}<svelte:component this={{{payload}}} />\n", script("")),
        // The `{@debug}` grammar admits identifiers only — the payload is the
        // bare mode-appropriate identifier (the maybe-runes marker is a plain
        // namespace import; a store const would be unadmitted without a `$s`
        // template read).
        S::DebugArg => match mode {
            WrapMode::DefiniteLegacy => format!("{}{{@debug obj}}\n<p>t</p>\n", script("")),
            WrapMode::MaybeRunes => {
                "<script>import * as NS from './x.js';</script>\n{@debug NS}\n<p>t</p>\n"
                    .to_string()
            }
        },
        // Official `SvelteBoundary.js` visits the attribute expression RAW (no
        // `build_expression`): a call-bearing `failed={…}` prop pins the raw
        // getter emission in definite legacy.
        S::BoundaryProp => format!(
            "{}<svelte:boundary failed={{{payload}}}><p>c</p></svelte:boundary>\n",
            script("")
        ),
    }
}
