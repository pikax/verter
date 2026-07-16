//! The Svelte IDE-projection ambient prelude.
//!
//! Every projected `.svelte.tsx` opens with one UNMAPPED prelude inserted at
//! output offset 0. It carries three things, none of which
//! shift a mapped position (the prelude is pure insertion — every original
//! script/template byte keeps its source offset):
//!
//! 1. the per-file `@jsxImportSource @verter/svelte-jsx` pragma — overriding
//!    the provider's project-level `jsxImportSource: "vue"` for THIS file only,
//!    even under `jsx: "preserve"`;
//! 2. the COMPLETE audited Svelte 5 rune surface as ambient `declare`s
//!    (`$props`/`$bindable`/`$state`/`$derived`/`$effect`/`$inspect`/`$host`,
//!    every namespace member, and `import type { Snippet } from "svelte"`) —
//!    rune CALL SITES stay verbatim, the prelude only TYPES them;
//! 3. the projection checkers/declarators: `__verter_attach` (the
//!    `{@attach}` target), the `__verter_snippet` brand declarator, and the
//!    `__verter_void` value checker (out-of-scope expressions route through it).
//!    `class` clsx forms (5.16) are typed by `SvelteHTMLElements`'
//!    `class?: ClassValue` in the intrinsic table — no separate class checker.
//!
//! The rune list is COMPLETE so no fixture using a namespace member fails the
//! clean-type-check gate spuriously. The declarations are ambient (`declare`)
//! so they introduce no runtime value and never collide with a user import.

use super::SvelteIdeDialect;

/// The per-file pragma line. Opens the prelude; overrides the project-level
/// `jsxImportSource` for this file only.
pub const PRAGMA_LINE: &str = "/** @jsxImportSource @verter/svelte-jsx */\n";

/// The JSX namespace a component projects into, selected by a top-level
/// `<svelte:options namespace="...">` (F10). The base HTML namespace is the
/// default; `svg`/`mathml` select the dedicated shim entrypoints whose
/// `IntrinsicElements` table is the svg / mathml element set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SvelteJsxNamespace {
    /// The default HTML (+ inline svg, per `SvelteHTMLElements`) namespace —
    /// `@verter/svelte-jsx`.
    #[default]
    Html,
    /// The svg namespace — `@verter/svelte-jsx/svg`.
    Svg,
    /// The mathml namespace — `@verter/svelte-jsx/mathml`.
    MathMl,
}

impl SvelteJsxNamespace {
    /// The `@jsxImportSource` pragma line for this namespace.
    #[must_use]
    pub fn pragma_line(self) -> &'static str {
        match self {
            Self::Html => PRAGMA_LINE,
            Self::Svg => "/** @jsxImportSource @verter/svelte-jsx/svg */\n",
            Self::MathMl => "/** @jsxImportSource @verter/svelte-jsx/mathml */\n",
        }
    }

    /// Classify a `<svelte:options namespace="...">` literal value. An unknown
    /// or absent value keeps the default HTML namespace.
    #[must_use]
    pub fn from_options_literal(value: &str) -> Self {
        match value {
            "svg" => Self::Svg,
            // Svelte accepts both `mathml` and the legacy `math`/`mathml` MathML
            // namespace literal; classify both to the mathml table.
            "mathml" | "math" => Self::MathMl,
            _ => Self::Html,
        }
    }
}

/// The render mode for [`render_rune_prelude`] — the SINGLE rune-declaration
/// source serves two surfaces.
///
/// * [`RunePreludeMode::Component`] — the full `.svelte` component IDE
///   projection prelude (the `@jsxImportSource` pragma, the COMPLETE rune
///   surface, every `__verter_*` projection checker, and — in legacy mode —
///   the `$$props`/`$$restProps`/`$$slots` magic). Output is BYTE-IDENTICAL to
///   the historical component prelude.
/// * [`RunePreludeMode::Module`] — the standalone rune-MODULE
///   (`.svelte.ts`/`.svelte.js`) prelude: ONLY the runes valid OUTSIDE a
///   component (`$state`/`$derived`/`$effect`/`$inspect` + their namespace
///   members). It EXCLUDES the component-only runes (`$props`/`$bindable`/
///   `$host`), the `Snippet`/`Attachment` imports, the `@jsxImportSource`
///   pragma, every `__verter_*` projection helper, and the legacy `$$` magic —
///   none of which exist in a non-component module. It carries a leading
///   `export {};` so the prepended declarations stay MODULE-LOCAL (a top-level
///   ambient `declare` in a script-context file would leak globally and the
///   runes would pollute every plain `.ts`/`.js`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunePreludeMode {
    /// The `.svelte` component IDE projection prelude (byte-identical to the
    /// historical component prelude).
    Component {
        /// The JSX namespace whose `@jsxImportSource` pragma opens the prelude.
        namespace: SvelteJsxNamespace,
        /// Whether to emit the F12 legacy magic objects (legacy/non-runes mode).
        legacy_mode: bool,
    },
    /// The standalone rune-module prelude. `source_type` selects the ambient
    /// form: a `.svelte.ts` module gets the TS `declare` form; a `.svelte.js`
    /// module (checked under `checkJs`) gets the JS-valid JSDoc-typed-function
    /// form, because TS `declare function` syntax is not valid JavaScript.
    Module {
        /// The module's script dialect.
        source_type: RuneModuleSourceType,
    },
}

/// The script dialect of a standalone rune module, selecting the JS-valid vs
/// TS ambient form of the module rune prelude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuneModuleSourceType {
    /// A `.svelte.ts` module — the TS `declare`-form rune prelude.
    Ts,
    /// A `.svelte.js` module — the JS-valid JSDoc-typed-function rune prelude.
    Js,
}

/// Render the complete UNMAPPED prelude text for a JSX `namespace`.
///
/// Thin wrapper over [`render_rune_prelude`] in [`RunePreludeMode::Component`]
/// — the `.svelte` component IDE projector's entry point. Kept for call-site
/// clarity; the output is byte-identical to the historical component prelude.
///
/// The result is a single block of inserted text. It is deterministic and
/// self-contained (every referenced type is imported or declared here), so a
/// fixture's only un-checked names are the user's own script/template
/// symbols. The leading `@jsxImportSource` pragma is the only part that varies
/// by namespace (F10) — the rune surface + checkers are namespace-invariant.
///
/// `legacy_mode` selects whether the F12 legacy magic-object declarations
/// (`$$props`/`$$restProps`/`$$slots`) are emitted. They exist ONLY in
/// legacy (non-runes) mode — a runes-mode component never references them, and
/// emitting their deliberately-loose `any`-typed surface there would pollute a
/// runes-mode file. The F11 store-subscription helpers
/// (`__verter_store_get`/`__verter_store_set`) are mode-INVARIANT (they are
/// only reached when the projector rewrote a real store-sub) and always emitted.
#[must_use]
pub fn render_prelude(namespace: SvelteJsxNamespace, legacy_mode: bool) -> String {
    render_rune_prelude(RunePreludeMode::Component {
        namespace,
        legacy_mode,
    })
}

/// Render the component prelude for the generated carrier dialect.
///
/// TypeScript carriers use ambient declarations. JavaScript carriers express
/// the same contracts as module-local JSDoc-typed functions and values so the
/// file is genuine JavaScript under `checkJs`, not TS syntax hidden behind a
/// `.jsx` name.
#[must_use]
pub(crate) fn render_component_prelude(
    namespace: SvelteJsxNamespace,
    legacy_mode: bool,
    dialect: SvelteIdeDialect,
) -> String {
    match dialect {
        SvelteIdeDialect::TypeScript => render_prelude(namespace, legacy_mode),
        SvelteIdeDialect::JavaScript => render_js_component_prelude(namespace, legacy_mode),
    }
}

/// Render the rune prelude for the given [`RunePreludeMode`] — the SINGLE
/// rune-declaration source for both the component IDE projection and the
/// standalone rune-module surface.
///
/// The module-valid rune declarations (`$state`/`$derived`/`$effect`/
/// `$inspect` + namespaces) are shared verbatim between the two modes; the
/// component mode wraps them with the pragma, the component-only runes, the
/// projection checkers, and the legacy magic, while the module mode emits ONLY
/// the module-valid subset under a module-local `export {};`.
#[must_use]
pub fn render_rune_prelude(mode: RunePreludeMode) -> String {
    match mode {
        RunePreludeMode::Component {
            namespace,
            legacy_mode,
        } => {
            // Byte-identical to the historical component prelude: pragma +
            // imports + component-only runes ($props/$bindable) + the shared
            // module runes ($state/$derived/$effect/$inspect) + $host + the
            // projection checkers + (legacy) magic objects.
            let pragma = namespace.pragma_line();
            let legacy = if legacy_mode {
                LEGACY_MAGIC_PRELUDE
            } else {
                ""
            };
            let mut out = String::with_capacity(
                pragma.len()
                    + COMPONENT_RUNE_IMPORTS_AND_HEADER.len()
                    + COMPONENT_ONLY_RUNES_PROPS_BINDABLE.len()
                    + SHARED_MODULE_RUNES.len()
                    + COMPONENT_ONLY_RUNE_HOST.len()
                    + COMPONENT_PROJECTION_CHECKERS.len()
                    + legacy.len(),
            );
            out.push_str(pragma);
            out.push_str(COMPONENT_RUNE_IMPORTS_AND_HEADER);
            out.push_str(COMPONENT_ONLY_RUNES_PROPS_BINDABLE);
            out.push_str(SHARED_MODULE_RUNES);
            out.push_str(COMPONENT_ONLY_RUNE_HOST);
            out.push_str(COMPONENT_PROJECTION_CHECKERS);
            out.push_str(legacy);
            out
        }
        RunePreludeMode::Module { source_type } => match source_type {
            // A module-local `export {};` keeps the prepended declarations from
            // leaking globally (a bare top-level `declare function` in a
            // script-context file becomes a global). The shared module runes
            // are the same audited surface the component prelude carries.
            RuneModuleSourceType::Ts => {
                let mut out = String::with_capacity(
                    MODULE_LOCAL_MARKER.len()
                        + MODULE_RUNE_HEADER.len()
                        + SHARED_MODULE_RUNES.len(),
                );
                out.push_str(MODULE_LOCAL_MARKER);
                out.push_str(MODULE_RUNE_HEADER);
                out.push_str(SHARED_MODULE_RUNES);
                out
            }
            RuneModuleSourceType::Js => {
                let mut out = String::with_capacity(
                    MODULE_LOCAL_MARKER.len() + MODULE_RUNE_HEADER_JS.len() + JS_MODULE_RUNES.len(),
                );
                out.push_str(MODULE_LOCAL_MARKER);
                out.push_str(MODULE_RUNE_HEADER_JS);
                out.push_str(JS_MODULE_RUNES);
                out
            }
        },
    }
}

/// The module-valid rune ambient declarations (TS `declare` form) WITHOUT the
/// module-local `export {};` marker or any header — the SHARED rune surface a
/// standalone rune module exposes.
///
/// This is the SAME [`SHARED_MODULE_RUNES`] the component and module preludes
/// carry. The session's per-file eval-environment merge (Channel A — so a rune
/// module's exported rune-derived types infer correctly through Verter's own
/// type-resolution engine) parses THIS text into an isolated env and merges its
/// symbols; the EvalEnv merge is already file-scoped, so it needs no
/// `export {};`. There is no second rune-declaration source.
#[must_use]
pub fn module_rune_ambient_source() -> &'static str {
    SHARED_MODULE_RUNES
}

/// The version of the rune ambient surface. Bumped whenever the module rune
/// declarations change so a prelude fix invalidates stale inferred exports of a
/// rune module (it enters the rune module's type/eval-env cache key).
pub const RUNE_AMBIENT_PRELUDE_VERSION: u32 = 1;

/// The component prelude's leading imports + rune-section header. Component
/// mode only — the `Snippet`/`Attachment` imports back the projection
/// checkers, which a non-component module never references.
const COMPONENT_RUNE_IMPORTS_AND_HEADER: &str = r#"import type { Snippet } from "svelte";
import type { Attachment } from "svelte/attachments";
// --- Svelte 5 runes (ambient; call sites stay verbatim) ---
"#;

/// The COMPONENT-ONLY runes that precede the shared module runes in source
/// order (`$props` + its namespace, `$bindable`). A standalone rune module
/// never calls these (they need a component instance), so the module prelude
/// omits them.
const COMPONENT_ONLY_RUNES_PROPS_BINDABLE: &str = r#"declare function $props<T = Record<string, unknown>>(): T;
declare namespace $props {
  function id(): string;
}
declare function $bindable<T = never>(fallback?: T): T;
"#;

/// The COMPONENT-ONLY `$host` rune (component custom-element host access),
/// emitted after the shared module runes in source order. Omitted from the
/// module prelude.
const COMPONENT_ONLY_RUNE_HOST: &str =
    "declare function $host<El extends HTMLElement = HTMLElement>(): El;\n";

/// The module-VALID rune surface — shared VERBATIM between the component and
/// module preludes. These are the runes Svelte 5 allows OUTSIDE a component
/// (`$state`/`$derived`/`$effect`/`$inspect` + every namespace member,
/// per the audit, 5.56.x). This is the ONE rune-declaration source;
/// neither mode re-declares them.
const SHARED_MODULE_RUNES: &str = r#"declare function $state<T>(initial: T): T;
declare function $state<T>(): T | undefined;
declare namespace $state {
  function raw<T>(initial: T): T;
  function raw<T>(): T | undefined;
  function snapshot<T>(state: T): T;
  function eager<T>(initial: T): T;
}
declare function $derived<T>(expression: T): T;
declare namespace $derived {
  function by<T>(fn: () => T): T;
}
declare function $effect(fn: () => void | (() => void)): void;
declare namespace $effect {
  function pre(fn: () => void | (() => void)): void;
  function tracking(): boolean;
  function root(fn: () => void | (() => void)): () => void;
  function pending(): boolean;
}
declare function $inspect<T extends unknown[]>(...values: T): { with: (fn: (type: "init" | "update", ...values: T) => void) => void };
declare namespace $inspect {
  function trace(name?: string): void;
}
"#;

/// The module-local scope marker. A standalone rune module's prepended
/// declarations MUST stay module-local: a bare top-level `declare function` in
/// a script-context file becomes a GLOBAL ambient, which would leak the runes
/// into every plain `.ts`/`.js` (req 4 — per-file scoping). `export {};`
/// forces the file into module context so the declarations are file-scoped.
const MODULE_LOCAL_MARKER: &str = "export {};\n";

/// The TS module-rune section header.
const MODULE_RUNE_HEADER: &str =
    "// --- Svelte 5 module runes (ambient; call sites stay verbatim) ---\n";

/// The JS module-rune section header.
const MODULE_RUNE_HEADER_JS: &str =
    "// --- Svelte 5 module runes (JSDoc-typed; call sites stay verbatim) ---\n";

/// The JS-VALID module rune surface for a `.svelte.js` module (checked under
/// `checkJs`). TS `declare function` syntax is not valid JavaScript, so the
/// runes are declared as JSDoc-typed local functions (module-local via the
/// leading `export {};`). The shapes mirror [`SHARED_MODULE_RUNES`] exactly so
/// `export const s = $state(0)` infers `number` identically to the `.ts` form.
const JS_MODULE_RUNES: &str = r#"/**
 * @template T
 * @overload
 * @param {T} initial
 * @returns {T}
 */
/**
 * @overload
 * @returns {unknown}
 */
/**
 * @template T
 * @param {T} [initial]
 * @returns {T | unknown}
 */
function $state(initial) {
  return initial;
}
/**
 * @template T
 * @overload
 * @param {T} initial
 * @returns {T}
 */
/**
 * @overload
 * @returns {unknown}
 */
/**
 * @template T
 * @param {T} [initial]
 * @returns {T | unknown}
 */
$state.raw = function (initial) {
  return initial;
};
/**
 * @template T
 * @param {T} state
 * @returns {T}
 */
$state.snapshot = function (state) {
  return state;
};
/**
 * @template T
 * @param {T} initial
 * @returns {T}
 */
$state.eager = function (initial) {
  return initial;
};
/**
 * @template T
 * @param {T} expression
 * @returns {T}
 */
function $derived(expression) {
  return expression;
}
/**
 * @template T
 * @param {() => T} fn
 * @returns {T}
 */
$derived.by = function (fn) {
  return fn();
};
/**
 * @param {() => (void | (() => void))} fn
 * @returns {void}
 */
function $effect(fn) {}
/**
 * @param {() => (void | (() => void))} fn
 * @returns {void}
 */
$effect.pre = function (fn) {};
/**
 * @returns {boolean}
 */
$effect.tracking = function () {
  return false;
};
/**
 * @param {() => (void | (() => void))} fn
 * @returns {() => void}
 */
$effect.root = function (fn) {
  return function () {};
};
/**
 * @returns {boolean}
 */
$effect.pending = function () {
  return false;
};
/**
 * @template {unknown[]} T
 * @param {T} values
 * @returns {{ with: (fn: (type: "init" | "update", ...values: T) => void) => void }}
 */
function $inspect(...values) {
  return {
    with: function (fn) {},
  };
}
/**
 * @param {string} [name]
 * @returns {void}
 */
$inspect.trace = function (name) {};
"#;

/// Render the JavaScript component prelude. The function bodies are inert IDE
/// witnesses; all framework contracts live in JSDoc so the carrier remains
/// valid JavaScript and TypeScript can still infer/check every generated call.
fn render_js_component_prelude(namespace: SvelteJsxNamespace, legacy_mode: bool) -> String {
    let legacy = if legacy_mode {
        JS_LEGACY_MAGIC_PRELUDE
    } else {
        ""
    };
    let mut out = String::with_capacity(
        namespace.pragma_line().len()
            + JS_COMPONENT_HEADER.len()
            + JS_COMPONENT_ONLY_RUNES_PROPS_BINDABLE.len()
            + JS_MODULE_RUNES.len()
            + JS_COMPONENT_ONLY_RUNE_HOST.len()
            + JS_COMPONENT_PROJECTION_CHECKERS.len()
            + legacy.len(),
    );
    out.push_str(namespace.pragma_line());
    out.push_str(JS_COMPONENT_HEADER);
    out.push_str(JS_COMPONENT_ONLY_RUNES_PROPS_BINDABLE);
    out.push_str(JS_MODULE_RUNES);
    out.push_str(JS_COMPONENT_ONLY_RUNE_HOST);
    out.push_str(JS_COMPONENT_PROJECTION_CHECKERS);
    out.push_str(legacy);
    out
}

const JS_COMPONENT_HEADER: &str = r#"// @ts-check
// --- Svelte 5 runes (JSDoc-typed; call sites stay verbatim) ---
"#;

const JS_COMPONENT_ONLY_RUNES_PROPS_BINDABLE: &str = r#"/**
 * @template [T=Record<string, unknown>]
 * @returns {T}
 */
function $props() {
  return /** @type {T} */ (/** @type {unknown} */ ({}));
}
/** @returns {string} */
$props.id = function () {
  return "";
};
/**
 * @template [T=never]
 * @param {T} [fallback]
 * @returns {T}
 */
function $bindable(fallback) {
  return /** @type {T} */ (fallback);
}
"#;

const JS_COMPONENT_ONLY_RUNE_HOST: &str = r#"/**
 * @template {HTMLElement} [El=HTMLElement]
 * @returns {El}
 */
function $host() {
  return /** @type {El} */ (/** @type {unknown} */ (null));
}
"#;

const JS_COMPONENT_PROJECTION_CHECKERS: &str = r#"// --- Verter projection checkers/declarators (JSDoc) ---
/**
 * @template {EventTarget} E
 * @param {import("svelte/attachments").Attachment<E>} attachment
 * @returns {void}
 */
function __verter_attach(attachment) {}
/**
 * @template {unknown[]} Params
 * @param {(...args: Params) => unknown} render
 * @returns {import("svelte").Snippet<Params>}
 */
function __verter_snippet(render) {
  return /** @type {import("svelte").Snippet<Params>} */ (/** @type {unknown} */ (render));
}
/** @param {...unknown} values @returns {void} */
function __verter_void(...values) {}
/**
 * @template {PromiseLike<unknown>} T
 * @param {T} value
 * @returns {Awaited<T>}
 */
function __verter_await_expr(value) {
  return /** @type {Awaited<T>} */ (/** @type {unknown} */ (value));
}
/**
 * @template {string} Tag
 * @typedef {Tag extends keyof HTMLElementTagNameMap ? HTMLElementTagNameMap[Tag] : Tag extends keyof SVGElementTagNameMap ? SVGElementTagNameMap[Tag] : Element} __VerterHostEl
 */
/**
 * @param {import("svelte/transition").TransitionConfig | ((options?: { direction: "in" | "out" }) => import("svelte/transition").TransitionConfig)} config
 * @returns {void}
 */
function __verter_transition(config) {}
/**
 * @param {import("svelte/animate").AnimationConfig | ((options?: { direction: "in" | "out" }) => import("svelte/animate").AnimationConfig)} config
 * @returns {void}
 */
function __verter_animate(config) {}
/** @template V @param {V} local @returns {V} */
function __verter_bind_rw(local) { return local; }
/** @template V @returns {V} */
function __verter_bind_read() { return /** @type {V} */ (/** @type {unknown} */ (undefined)); }
/**
 * @template Host
 * @template {Host} To
 * @param {Host} host
 * @param {To} local
 * @returns {void}
 */
function __verter_bind_this_assignable(host, local) {}
/** @template {readonly unknown[]} L @param {L} local @returns {L} */
function __verter_bind_group_checkbox(local) { return local; }
/**
 * @template L
 * @param {L extends readonly unknown[] ? never : L} local
 * @returns {L}
 */
function __verter_bind_group_radio(local) { return /** @type {L} */ (local); }
/**
 * @template V
 * @param {(() => V) | null} get
 * @param {(value: V) => void} set
 * @returns {void}
 */
function __verter_bind_fn(get, set) {}
/**
 * @template V
 * @param {null} get
 * @param {(value: V) => void} set
 * @returns {void}
 */
function __verter_bind_fn_read(get, set) {}
/**
 * @template C
 * @typedef {C extends import("svelte").Component<infer P extends Record<string, any>, any, any> ? P : C extends { new (...args: any[]): { $props: infer P } } ? P : never} __VerterComponentProps
 */
/**
 * @template C
 * @typedef {C extends import("svelte").Component<any, infer E extends Record<string, any>, any> ? E : C extends { new (...args: any[]): infer I } ? I : never} __VerterComponentExports
 */
/**
 * @template C
 * @typedef {C extends import("svelte").Component<any, any, infer B extends PropertyKey> ? Exclude<B, ""> & string : keyof __VerterComponentProps<C> & string} __VerterComponentBindings
 */
/**
 * @template C
 * @template {__VerterComponentBindings<C>} K
 * @param {C} component
 * @param {K} name
 * @returns {void}
 */
function __verter_component_binding(component, name) {}
/**
 * @template C
 * @param {C} component
 * @returns {C extends import("svelte").Component<any, any, any> ? C : C extends { new (...args: any[]): { $props: infer P extends Record<string, any> } } ? import("svelte").Component<P, __VerterComponentExports<C> & Record<string, any>, ""> : never}
 */
function __verter_component(component) {
  return /** @type {C extends import("svelte").Component<any, any, any> ? C : C extends { new (...args: any[]): { $props: infer P extends Record<string, any> } } ? import("svelte").Component<P, __VerterComponentExports<C> & Record<string, any>, ""> : never} */ (/** @type {unknown} */ (component));
}
/**
 * @template {import("svelte").Component<any, any, any> | { new (...args: any[]): { $props: any } }} C
 * @param {C} component
 * @returns {(props: __VerterComponentProps<C> & { children?: unknown }) => ReturnType<import("svelte").Snippet>}
 */
function __verter_dynamic_component(component) {
  return /** @type {(props: __VerterComponentProps<C> & { children?: unknown }) => ReturnType<import("svelte").Snippet>} */ (
    function (props) { return /** @type {ReturnType<import("svelte").Snippet>} */ (/** @type {unknown} */ (null)); }
  );
}
/**
 * @template C
 * @typedef {(C extends { new (...args: any[]): infer I } ? I : never) extends { $events: infer E } ? E : {}} __VerterLegacyEventsOf
 */
/**
 * @template C
 * @typedef {{ [K in keyof __VerterComponentProps<C> & string]: K extends `on${infer E}` ? E : never }[keyof __VerterComponentProps<C> & string] | (keyof __VerterLegacyEventsOf<C> & string)} __VerterEventNames
 */
/**
 * @template C
 * @template {__VerterEventNames<C>} K
 * @typedef {K extends keyof __VerterLegacyEventsOf<C> ? __VerterLegacyEventsOf<C>[K] : `on${K}` extends keyof __VerterComponentProps<C> ? NonNullable<__VerterComponentProps<C>[`on${K}`]> : never} __VerterEventHandler
 */
/**
 * @template C
 * @template {__VerterEventNames<C>} K
 * @param {C} component
 * @param {K} name
 * @param {__VerterEventHandler<C, K>} handler
 * @returns {void}
 */
function __verter_event(component, name, handler) {}
/** @template T @param {import("svelte/store").Readable<T>} store @returns {T} */
function __verter_store_get(store) { return /** @type {T} */ (/** @type {unknown} */ (undefined)); }
/**
 * @template T
 * @param {import("svelte/store").Writable<T>} store
 * @param {T} value
 * @returns {T}
 */
function __verter_store_set(store, value) { return value; }
/**
 * @template T
 * @param {import("svelte/store").Writable<T>} store
 * @returns {{ value: T }}
 */
function __verter_store_lvalue(store) { return /** @type {{ value: T }} */ ({ value: undefined }); }
/** @template {number | bigint} T @param {T} current @returns {T} */
function __verter_store_update(current) { return current; }
"#;

const JS_LEGACY_MAGIC_PRELUDE: &str = r#"// --- F12 legacy magic objects (legacy-mode only; JSDoc-typed).
const $$props = /** @type {Record<string, any>} */ ({});
const $$restProps = /** @type {Record<string, any>} */ ({});
const $$slots = /** @type {Record<string, boolean>} */ ({});
"#;

/// The component projection checkers/declarators section — the
/// `__verter_*` helpers + the host-element / event helper types. Component
/// mode only; a non-component module never references any of them.
const COMPONENT_PROJECTION_CHECKERS: &str = r#"// --- Verter projection checkers/declarators ---
declare function __verter_attach<E extends EventTarget>(attachment: Attachment<E>): void;
declare function __verter_snippet<Params extends unknown[]>(render: (...args: Params) => unknown): Snippet<Params>;
declare function __verter_void(...values: unknown[]): void;
// F6 experimental await-EXPRESSION projection (`{await e}` in markup / inside a
// rune). `__verter_render` STAYS SYNC — a markup `await e` is rewritten to
// `__verter_await_expr(e)` so the resolved value type flows to the use site
// (`Awaited<T>` is resolved by TS/the shared type path, NO Svelte promise
// walker). The `T extends PromiseLike<unknown>` constraint makes `{await 1}`
// (a non-promise) FAIL type-checking, while `{await fetchUser()}` flows
// `Awaited<typeof fetchUser()>`.
declare function __verter_await_expr<T extends PromiseLike<unknown>>(value: T): Awaited<T>;
// The host-element instance type for a projected tag (the `transition:`/`in:`/
// `out:`/`animate:` host node). A known HTML/SVG tag resolves to its
// precise DOM instance type; an unknown/custom-element/dynamic host falls back
// to `Element`. The projector emits the host node as `(null! as
// __VerterHostEl<"tag">)` and CALLS the directive's transition/animate function
// on it — a real call site is the soundest check (TSGO checks the host-node
// type, the params type, the arg count, and that the value is callable; the
// generic-checker indirection loses params discrimination under contravariant
// optional-param inference, so the projection calls the function directly).
type __VerterHostEl<Tag extends string> =
  Tag extends keyof HTMLElementTagNameMap ? HTMLElementTagNameMap[Tag] :
  Tag extends keyof SVGElementTagNameMap ? SVGElementTagNameMap[Tag] :
  Element;
// `transition:fn={p}` / `in:` / `out:` — the RESULT-SHAPE checker. The projector
// CALLS the transition function `fn(node, params)` (the host-node / params /
// arity / non-function checks happen at that real call site) and routes the call
// RESULT through this checker, which asserts `fn` returned a `TransitionConfig`
// or a DEFERRED transition factory. Svelte invokes a deferred factory with the
// runtime `{ direction }` descriptor, so the factory shape accepts an OPTIONAL
// options argument (`(options?: { direction }) => TransitionConfig`) — a
// zero-arg `() => TransitionConfig` and a direction-consuming factory both
// satisfy it. A `fn` returning the wrong shape FAILS.
declare function __verter_transition(config: import("svelte/transition").TransitionConfig | ((options?: { direction: "in" | "out" }) => import("svelte/transition").TransitionConfig)): void;
// `animate:fn={p}` — the RESULT-SHAPE checker. The projector CALLS the animate
// function on the host node + from/to rects + params and asserts the result is
// an `AnimationConfig` (or a deferred factory of one).
declare function __verter_animate(config: import("svelte/animate").AnimationConfig | ((options?: { direction: "in" | "out" }) => import("svelte/animate").AnimationConfig)): void;
// --- F4 wide `bind:` family value-type checkers (the bind-contract table is the
// AUTHORITY; these are implementation helpers). The projector emits an
// assignment through one of these so the bound LOCAL is checked against the
// binding's value type `V` from the table, in the binding's DIRECTION:
//   read-write (invariant): `LOCAL = __verter_bind_rw<V>(LOCAL)` — the arg checks
//     `LOCAL` assignable to `V` (local → DOM), the return `V` assigns back into
//     `LOCAL` (DOM → local); together invariant, and a `const` target fails.
//   read (readonly DOM → local): `LOCAL = __verter_bind_read<V>()` — `V` assigns
//     into `LOCAL`; a `const`/wrong-typed target FAILS (the write-rejection the
//     readonly fixture pins).
declare function __verter_bind_rw<V>(local: V): V;
declare function __verter_bind_read<V>(): V;
// `bind:this={el}` invariance (F4). The local is commonly declared WITHOUT an
// initializer (`let el: HTMLInputElement`), so the check must NOT read its
// value. The projector emits `(LOCAL = (null! as Host))` (writes Host into the
// local — the `Host extends typeof LOCAL` direction + definite assignment) paired
// with `__verter_bind_this_assignable<Host, typeof LOCAL>()`, whose constraint
// `To extends Host` asserts `typeof LOCAL extends Host` (the OTHER direction) at
// the TYPE level — together invariant, discriminating a wrong element type
// (DOM element instance types are largely mutually assignable, so a one-way
// check would not), without reading the local's (possibly unassigned) value.
declare function __verter_bind_this_assignable<Host, To extends Host>(): void;
// `bind:group` — checkbox shares ONE array variable, radio shares ONE item
// variable. The checkbox checker requires the local be an array (a loose
// `T | T[]` union is NOT `extends readonly unknown[]` → rejected). The radio
// checker requires the local be a NON-array: the DISTRIBUTIVE conditional
// `L extends readonly unknown[] ? never : L` maps each union arm independently,
// so a `T | T[]` union narrows the expected param to `T` and the `T[]` arm of
// the supplied local is NOT assignable → rejected (a NON-distributive
// `[L] extends [readonly unknown[]]` would wrongly accept the union). Both
// round-trip (group is read-write), so a `const` target also fails.
declare function __verter_bind_group_checkbox<L extends readonly unknown[]>(local: L): L;
declare function __verter_bind_group_radio<L>(local: L extends readonly unknown[] ? never : L): L;
// --- F5 function bindings `bind:x={get, set}` (and write-only `{null, set}`).
// The checker enforces get/set mutual consistency against the bind-target type
// `V`: `get` returns `V` (or `null` for write-only), `set` consumes `V`. For an
// element bind the projector passes `V` from the bind-contract table; for a
// component bind it passes `__VerterComponentProps<typeof Child>[K]` (the typing
// is done in the PROJECTED TSX via TS — no Rust resolver call). A READONLY
// element binding routes to `__verter_bind_fn_read`, whose `get` is `null`-only
// (a readonly function binding must be the write-only `{null, set}` form).
declare function __verter_bind_fn<V>(get: (() => V) | null, set: (value: V) => void): void;
declare function __verter_bind_fn_read<V>(get: null, set: (value: V) => void): void;
type __VerterComponentProps<C> =
  C extends import("svelte").Component<infer P extends Record<string, any>, any, any> ? P :
  C extends abstract new (...args: never[]) => { $props: infer P } ? P :
  never;
type __VerterComponentExports<C> =
  C extends import("svelte").Component<any, infer E extends Record<string, any>, any> ? E :
  C extends abstract new (...args: never[]) => infer I ? I :
  never;
type __VerterComponentBindings<C> =
  C extends import("svelte").Component<any, any, infer B extends PropertyKey> ? Exclude<B, ""> & string :
  keyof __VerterComponentProps<C> & string;
declare function __verter_component_binding<
  C,
  K extends __VerterComponentBindings<C>
>(component: C, name: K): void;
declare function __verter_component<C>(component: C):
  C extends import("svelte").Component<any, any, any> ? C :
  C extends abstract new (...args: never[]) => { $props: infer P extends Record<string, any> }
    ? import("svelte").Component<P, __VerterComponentExports<C> & Record<string, any>, "">
    : never;
// --- F8 `<svelte:component this={C}>` / `<svelte:self>` dynamic component.
// The `this` value may be a native callable Svelte 5 Component or the private
// class-shaped foreign-component adapter. Props are extracted through
// `__VerterComponentProps<C>`, and the helper returns a JSX function component
// typed by those props. A wrong prop or non-component `this` fails. `children` is
// permitted (svelte components accept slotted children) without forcing it onto
// the `$props` contract. The return type is `ReturnType<Snippet>` (the
// projected-element shape), never the pragma-bound `JSX.Element` (which is not
// in lexical scope in the prelude).
declare function __verter_dynamic_component<
  C extends import("svelte").Component<any, any, any> | (abstract new (...args: never[]) => { $props: any })
>(component: C): (props: __VerterComponentProps<C> & { children?: unknown }) => ReturnType<Snippet>;
// --- F13 component `on:event={handler}` payload checking. A COMPONENT element's
// `on:select={h}` projects to `{...(__verter_event(Child, "select", h), {})}` —
// the helper resolves native Svelte 5 `on${event}` callback props. A private
// `$events` fallback remains only for class-shaped foreign components. Unknown
// names and wrong handler payloads fail; no untyped event bag is introduced.
type __VerterLegacyEventsOf<C> =
  (C extends new (...args: any[]) => infer I ? I : never) extends { $events: infer E } ? E : {};
type __VerterCallbackEventNames<C> = {
  [K in keyof __VerterComponentProps<C> & string]: K extends `on${infer E}` ? E : never
}[keyof __VerterComponentProps<C> & string];
type __VerterEventNames<C> = __VerterCallbackEventNames<C> | (keyof __VerterLegacyEventsOf<C> & string);
type __VerterEventHandler<C, K extends __VerterEventNames<C>> =
  K extends keyof __VerterLegacyEventsOf<C> ? __VerterLegacyEventsOf<C>[K] :
  `on${K}` extends keyof __VerterComponentProps<C> ? NonNullable<__VerterComponentProps<C>[`on${K}`]> :
  never;
declare function __verter_event<C, K extends __VerterEventNames<C>>(
  component: C,
  name: K,
  handler: __VerterEventHandler<C, K>,
): void;
// --- F11 store auto-subscription (`$store`) helpers. The projector rewrites
// ONLY the `$` byte / `=` operator spans of a classified store-sub, preserving
// the original `store` identifier / RHS bytes (sourcemap accuracy). A READ
// `$store` becomes `__verter_store_get(store)` — typed `T` from the store's
// `Readable<T>` (a non-store `store` FAILS the constraint). A WRITE
// `$store = v` becomes `__verter_store_set(store, v)` — `T` is the store's
// `Writable<T>` value type and `v` is checked against it (a readonly
// `Readable<T>` store FAILS, a wrong-typed `v` FAILS). `__verter_store_set`
// returns `T` so the rewrite stays a valid expression in any assignment
// position.
declare function __verter_store_get<T>(store: import("svelte/store").Readable<T>): T;
declare function __verter_store_set<T>(store: import("svelte/store").Writable<T>, value: T): T;
// A `$store` in an lvalue DESTRUCTURING / `for`-of WRITE TARGET (`[$store] =
// xs` / `({ x: $store } = obj)` / `for ($store of xs)`) projects to
// `__verter_store_lvalue(store).value` — a VALID assignment-target member access
// referencing only the `store` local. The `{ value: T }` carrier checks the
// destructured / iterated element against the store's `Writable<T>` value type
// (a non-`Writable` store FAILS the constraint, a wrong-typed element FAILS the
// `.value` assignment) without raw `$store` residue. A function-call result is
// not a valid lvalue, so the helper returns the carrier OBJECT whose `.value`
// member IS the lvalue.
declare function __verter_store_lvalue<T>(store: import("svelte/store").Writable<T>): { value: T };
// `$store++` / `--$store` UPDATE: the increment/decrement operand must be
// `number | bigint` (TS rejects `++` on a string/boolean). Modelling the update
// as `get(store) + 1` would FALSELY reject a `bigint` store (`bigint + number`
// is an error) and FALSELY accept a `string` store (string concatenation). This
// helper enforces the exact `++`/`--` operand constraint while PRESERVING the
// value type `T` (numeric literal / `number` / `bigint` / numeric enum) — the
// projector emits `__verter_store_set(store, __verter_store_update(
// __verter_store_get(store)))`.
declare function __verter_store_update<T extends number | bigint>(current: T): T;
"#;

/// The F12 legacy magic-object declarations (`$$props`/`$$restProps`/`$$slots`).
///
/// Emitted ONLY in legacy (non-runes) mode — the magic objects do not exist in
/// runes mode. The `Record<string, any>` typing of `$$props`/`$$restProps` is
/// an EXPLICIT, OWNER-APPROVED anti-`any`-gate exception scoped to the legacy
/// magic object itself: Svelte's legacy `$$props`/`$$restProps` are
/// intrinsically untyped bags of forwarded attributes, so a precise type is not
/// recoverable. This carve-out applies to NOTHING else — every other projected
/// surface stays precisely typed. `$$slots` is `Record<string, boolean>`
/// (`$$slots.foo` is `boolean`: whether the `foo` slot was filled), which is a
/// precise type, not the `any` exception.
const LEGACY_MAGIC_PRELUDE: &str = r#"// --- F12 legacy magic objects (legacy-mode only; ambient `declare const`).
// ANTI-`any`-GATE EXCEPTION (owner-approved): `$$props`/`$$restProps` are the
// legacy forwarded-attribute bag, intrinsically untyped — the deliberate `any`
// is scoped to THESE TWO declarations ONLY. `$$slots` is precisely typed.
declare const $$props: Record<string, any>;
declare const $$restProps: Record<string, any>;
declare const $$slots: Record<string, boolean>;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_opens_with_the_jsx_import_source_pragma() {
        let prelude = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(prelude.starts_with("/** @jsxImportSource @verter/svelte-jsx */"));
    }

    #[test]
    fn prelude_pragma_varies_by_namespace() {
        // F10: the svg / mathml namespaces select the dedicated shim entrypoints
        // via the leading pragma; the rune surface stays namespace-invariant.
        let html = render_prelude(SvelteJsxNamespace::Html, true);
        let svg = render_prelude(SvelteJsxNamespace::Svg, true);
        let mathml = render_prelude(SvelteJsxNamespace::MathMl, true);
        assert!(html.starts_with("/** @jsxImportSource @verter/svelte-jsx */"));
        assert!(svg.starts_with("/** @jsxImportSource @verter/svelte-jsx/svg */"));
        assert!(mathml.starts_with("/** @jsxImportSource @verter/svelte-jsx/mathml */"));
        // The body after the pragma is identical across namespaces.
        let body_of = |s: &str| s[s.find('\n').unwrap() + 1..].to_string();
        assert_eq!(body_of(&svg), body_of(&html));
        assert_eq!(body_of(&mathml), body_of(&html));
    }

    #[test]
    fn namespace_classifies_the_options_literal() {
        assert_eq!(
            SvelteJsxNamespace::from_options_literal("svg"),
            SvelteJsxNamespace::Svg
        );
        assert_eq!(
            SvelteJsxNamespace::from_options_literal("mathml"),
            SvelteJsxNamespace::MathMl
        );
        // An absent / unknown / `html` literal keeps the default HTML namespace.
        assert_eq!(
            SvelteJsxNamespace::from_options_literal("html"),
            SvelteJsxNamespace::Html
        );
        assert_eq!(
            SvelteJsxNamespace::from_options_literal(""),
            SvelteJsxNamespace::Html
        );
    }

    #[test]
    fn prelude_declares_the_dynamic_component_checker() {
        // F8 accepts native Svelte 5 Components and the private class-shaped
        // foreign-component adapter, extracting props through one helper.
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(p.contains("declare function __verter_dynamic_component"));
        assert!(
            p.contains("import(\"svelte\").Component<any, any, any> | (abstract new"),
            "the dynamic checker accepts native and private class-shaped components"
        );
        assert!(
            p.contains("(props: __VerterComponentProps<C> & { children?: unknown })"),
            "the returned function component uses the native-aware props extractor"
        );
    }

    #[test]
    fn prelude_declares_the_component_event_helper() {
        // F13 resolves Svelte 5 callback props, retaining the private legacy
        // `$events` fallback for foreign class-shaped components.
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(p.contains("type __VerterCallbackEventNames<C>"));
        assert!(p.contains("type __VerterLegacyEventsOf<C>"));
        assert!(p.contains("declare function __verter_event<C, K extends __VerterEventNames<C>>"));
        assert!(
            p.contains("handler: __VerterEventHandler<C, K>"),
            "the handler is typed by the native callback or legacy event payload"
        );
        // The loose `CustomEvent<any>` projection is NOT present.
        assert!(
            !p.contains("CustomEvent<any>"),
            "no loose CustomEvent<any> in the F13 event surface"
        );
    }

    #[test]
    fn prelude_declares_the_complete_rune_surface() {
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        for needle in [
            "declare function $props",
            "function id()",
            "declare function $bindable",
            "declare function $state",
            "function raw<",
            "function snapshot<",
            "function eager<",
            "declare function $derived",
            "function by<",
            "declare function $effect",
            "function pre(",
            "function tracking()",
            "function root(",
            "function pending()",
            "declare function $inspect",
            "function trace(",
            "declare function $host",
        ] {
            assert!(p.contains(needle), "prelude missing rune member: {needle}");
        }
    }

    #[test]
    fn prelude_declares_the_three_checkers_and_imports_snippet() {
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(p.contains("import type { Snippet } from \"svelte\""));
        assert!(p.contains("declare function __verter_attach"));
        assert!(p.contains("declare function __verter_snippet"));
        assert!(p.contains("declare function __verter_void"));
        // The `class` clsx form (5.16) is typed by `SvelteHTMLElements`'
        // `class?: ClassValue` in the intrinsic table — no dead
        // `__verter_class` declarator (NIT-1).
        assert!(!p.contains("__verter_class"), "dead declarator removed");
    }

    #[test]
    fn prelude_declares_the_await_expression_helper() {
        // F6: the experimental await-EXPRESSION helper is declared with the
        // PromiseLike constraint so a non-promise (`{await 1}`) FAILS while a
        // promise flows `Awaited<T>`. `__verter_render` stays SYNC.
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(
            p.contains(
                "declare function __verter_await_expr<T extends PromiseLike<unknown>>(value: T): Awaited<T>;"
            ),
            "the PromiseLike-constrained await-expression helper is declared: {p}"
        );
    }

    #[test]
    fn prelude_declares_the_transition_and_animate_checkers() {
        // F2/F3: the `transition:`/`in:`/`out:` and `animate:` RESULT-SHAPE
        // checkers are declared referencing the `svelte/transition`/
        // `svelte/animate` config types, plus the `__VerterHostEl<Tag>` host-node
        // helper the projector uses for the real call site.
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(p.contains("declare function __verter_transition"));
        assert!(p.contains("declare function __verter_animate"));
        assert!(p.contains("type __VerterHostEl"));
        assert!(
            p.contains("import(\"svelte/transition\").TransitionConfig"),
            "transition config referenced"
        );
        assert!(
            p.contains("import(\"svelte/animate\").AnimationConfig"),
            "animate config referenced"
        );
        // The host-node helper maps a known tag to its precise DOM instance type
        // and falls back to `Element` for unknown/dynamic hosts.
        assert!(
            p.contains("Tag extends keyof HTMLElementTagNameMap"),
            "host-node helper resolves known HTML tags"
        );
        assert!(
            p.contains("Tag extends keyof SVGElementTagNameMap"),
            "host-node helper resolves known SVG tags"
        );
    }

    #[test]
    fn prelude_declares_the_bind_family_checkers() {
        // F4/F5: the wide `bind:` family value-type checkers (read-write / read /
        // write), the `bind:group` checkbox/radio checkers, and the F5
        // function-binding checker are all declared.
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        for needle in [
            "declare function __verter_bind_rw",
            "declare function __verter_bind_read",
            "declare function __verter_bind_this_assignable",
            "declare function __verter_bind_group_checkbox",
            "declare function __verter_bind_group_radio",
            "declare function __verter_bind_fn",
            "declare function __verter_bind_fn_read",
        ] {
            assert!(p.contains(needle), "prelude missing bind checker: {needle}");
        }
    }

    #[test]
    fn prelude_declares_the_f11_store_subscription_helpers() {
        // F11: the store-get (`Readable<T>` → `T`) and store-set (`Writable<T>`
        // checked) helpers are declared in BOTH modes (mode-invariant — they are
        // only reached when the projector rewrote a real store-sub).
        for legacy in [true, false] {
            let p = render_prelude(SvelteJsxNamespace::Html, legacy);
            assert!(
                p.contains(
                    "declare function __verter_store_get<T>(store: import(\"svelte/store\").Readable<T>): T;"
                ),
                "the store-get helper reads `T` from `Readable<T>`"
            );
            assert!(
                p.contains(
                    "declare function __verter_store_set<T>(store: import(\"svelte/store\").Writable<T>, value: T): T;"
                ),
                "the store-set helper checks `value` against the store's `Writable<T>`"
            );
            assert!(
                p.contains(
                    "declare function __verter_store_update<T extends number | bigint>(current: T): T;"
                ),
                "the store-update helper enforces the `++`/`--` `number | bigint` operand"
            );
            assert!(
                p.contains(
                    "declare function __verter_store_lvalue<T>(store: import(\"svelte/store\").Writable<T>): { value: T };"
                ),
                "the store-lvalue helper exposes a writable `{{ value: T }}` carrier for \
                 destructuring / for-of write targets"
            );
        }
    }

    #[test]
    fn legacy_mode_declares_the_f12_magic_objects_with_the_documented_any_exception() {
        // F12: in LEGACY mode the magic objects are declared. The OWNER-APPROVED
        // anti-`any`-gate exception is scoped to `$$props`/`$$restProps` ONLY;
        // `$$slots` is the precise `Record<string, boolean>`.
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(p.contains("declare const $$props: Record<string, any>;"));
        assert!(p.contains("declare const $$restProps: Record<string, any>;"));
        assert!(p.contains("declare const $$slots: Record<string, boolean>;"));
        // The exception is DOCUMENTED at the declaration site.
        assert!(
            p.contains("ANTI-`any`-GATE EXCEPTION"),
            "the legacy `any` carve-out is documented at the declaration site"
        );
    }

    #[test]
    fn component_mode_render_is_byte_identical_to_the_historical_prelude() {
        // The refactor into ONE rune-declaration source (Component/Module modes)
        // must NOT change a single byte of the component prelude. The
        // `render_prelude` wrapper and `render_rune_prelude(Component {..})` are
        // the same output, and the historical fragment-concatenation is exact.
        for namespace in [
            SvelteJsxNamespace::Html,
            SvelteJsxNamespace::Svg,
            SvelteJsxNamespace::MathMl,
        ] {
            for legacy_mode in [true, false] {
                let via_wrapper = render_prelude(namespace, legacy_mode);
                let via_mode = render_rune_prelude(RunePreludeMode::Component {
                    namespace,
                    legacy_mode,
                });
                assert_eq!(
                    via_wrapper, via_mode,
                    "the wrapper and the Component mode must produce identical bytes"
                );
                // The historical layout: pragma, then the imports, then the rune
                // surface in source order, then the checkers, then (legacy) magic.
                assert!(via_mode.starts_with(namespace.pragma_line()));
                assert!(via_mode.contains("import type { Snippet } from \"svelte\""));
                // The component-only runes precede $state in source order.
                let props_at = via_mode.find("declare function $props").unwrap();
                let state_at = via_mode.find("declare function $state").unwrap();
                let host_at = via_mode.find("declare function $host").unwrap();
                let checker_at = via_mode.find("declare function __verter_attach").unwrap();
                assert!(props_at < state_at, "$props precedes $state");
                assert!(state_at < host_at, "$state precedes $host");
                assert!(
                    host_at < checker_at,
                    "$host precedes the projection checkers"
                );
            }
        }
    }

    #[test]
    fn ts_rune_module_prelude_is_the_module_local_module_rune_subset() {
        // A `.svelte.ts` rune module gets ONLY the module-valid runes
        // ($state/$derived/$effect/$inspect + namespaces), module-local via a
        // leading `export {};` so the declarations do not leak globally.
        let p = render_rune_prelude(RunePreludeMode::Module {
            source_type: RuneModuleSourceType::Ts,
        });
        // Module-local marker FIRST (per-file scoping — no global rune leak).
        assert!(
            p.starts_with("export {};\n"),
            "the module prelude opens with `export {{}};` for module-local scope: {p}"
        );
        // The full module-valid rune surface (TS `declare` form).
        for needle in [
            "declare function $state<T>(initial: T): T;",
            "function raw<",
            "function snapshot<",
            "function eager<",
            "declare function $derived",
            "function by<",
            "declare function $effect",
            "function pre(",
            "function tracking()",
            "function root(",
            "function pending()",
            "declare function $inspect",
            "function trace(",
        ] {
            assert!(
                p.contains(needle),
                "module prelude missing rune member: {needle}"
            );
        }
        // DISCRIMINATING EXCLUSIONS: component-only runes, the imports, the
        // pragma, the projection checkers, and the legacy magic are ALL absent.
        assert!(!p.contains("$props"), "no $props in a rune module");
        assert!(!p.contains("$bindable"), "no $bindable in a rune module");
        assert!(!p.contains("$host"), "no $host in a rune module");
        assert!(
            !p.contains("@jsxImportSource"),
            "no jsx pragma in a rune module"
        );
        assert!(
            !p.contains("import type { Snippet }"),
            "no Snippet import in a rune module"
        );
        assert!(
            !p.contains("import type { Attachment }"),
            "no Attachment import in a rune module"
        );
        assert!(
            !p.contains("__verter_"),
            "no projection checkers in a rune module"
        );
        assert!(!p.contains("$$props"), "no legacy magic in a rune module");
        assert!(!p.contains("$$slots"), "no legacy magic in a rune module");
    }

    #[test]
    fn js_rune_module_prelude_is_js_valid_and_module_local() {
        // A `.svelte.js` rune module (checked under checkJs) gets the
        // JS-valid JSDoc-typed-function form — TS `declare function` is not
        // valid JS. Same rune surface, module-local.
        let p = render_rune_prelude(RunePreludeMode::Module {
            source_type: RuneModuleSourceType::Js,
        });
        assert!(p.starts_with("export {};\n"));
        // JS-valid: NO TS `declare` syntax anywhere.
        assert!(
            !p.contains("declare "),
            "the JS module prelude must not use TS `declare` syntax: {p}"
        );
        // The runes are JSDoc-typed local functions inferring the same shapes.
        // Multi-line JSDoc with `@template`/`@param`/`@returns` on their own
        // lines is REQUIRED — a single-line `@template T @param {T} ...` does
        // not bind the generic under strict checkJs (TSGO infers `any`).
        assert!(p.contains("function $state(initial) {"));
        assert!(p.contains("$state.raw = function"));
        assert!(p.contains("function $derived(expression) {"));
        assert!(p.contains("$derived.by = function"));
        assert!(p.contains("function $effect(fn) {}"));
        assert!(p.contains("$effect.pre = function"));
        assert!(p.contains("$effect.tracking = function"));
        assert!(p.contains("$effect.root = function"));
        assert!(p.contains("$effect.pending = function"));
        assert!(p.contains("function $inspect(...values)"));
        assert!(p.contains("@template T"), "JSDoc generic for inference");
        // The generic JSDoc tags are on their OWN lines (binds under checkJs).
        // `$state.eager` keeps the single-overload `@param {T} initial` form.
        assert!(
            p.contains("/**\n * @template T\n * @param {T} initial\n * @returns {T}\n */"),
            "multi-line JSDoc binds the generic under strict checkJs: {p}"
        );
        // PARITY with the TS surface: `$state` and `$state.raw` carry the
        // zero-arg overload. A required-arg-only JS surface would FAIL a valid
        // zero-arg `let count = $state()`. The zero-arg overload returns
        // `unknown` (NOT a generic `T | undefined`): a JS call site has no place
        // to bind an unconstrained `T`, so a generic return would collapse to
        // the UNSOUND `any` under checkJs; the TS form `$state<T>(): T |
        // undefined` resolves its unbound `T` to `unknown`, so the faithful JS
        // mirror returns `unknown` directly (sound, not `any`).
        assert!(
            p.contains("/**\n * @overload\n * @returns {unknown}\n */"),
            "the zero-arg `$state()` / `$state.raw()` overload returns `unknown` \
             (the sound TS mirror, not the unsound generic-collapses-to-`any`): {p}"
        );
        // The `@overload` tag is present (TS JSDoc overload mechanism).
        assert!(
            p.contains("@overload"),
            "JSDoc @overload for the zero-arg form"
        );
        // Same exclusions as the TS form.
        assert!(!p.contains("$props"));
        assert!(!p.contains("$bindable"));
        assert!(!p.contains("$host"));
        assert!(!p.contains("__verter_"));
        assert!(!p.contains("@jsxImportSource"));
    }

    #[test]
    fn runes_mode_omits_the_f12_magic_objects() {
        // F12: in RUNES mode the magic objects do NOT exist — their declarations
        // (and the loose `any`) are OMITTED so a runes-mode file stays clean.
        let p = render_prelude(SvelteJsxNamespace::Html, false);
        assert!(
            !p.contains("declare const $$props"),
            "no $$props in runes mode"
        );
        assert!(
            !p.contains("declare const $$restProps"),
            "no $$restProps in runes mode"
        );
        assert!(
            !p.contains("declare const $$slots"),
            "no $$slots in runes mode"
        );
        // And no stray `any` carve-out leaks into runes mode.
        assert!(
            !p.contains("ANTI-`any`-GATE EXCEPTION"),
            "the legacy `any` carve-out is legacy-mode only"
        );
    }
}
