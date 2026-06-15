//! The Svelte IDE-projection ambient prelude.
//!
//! Every projected `.svelte.tsx` opens with one UNMAPPED prelude inserted at
//! output offset 0 (D-u / D-ad / D-ae). It carries three things, none of which
//! shift a mapped position (the prelude is pure insertion — every original
//! script/template byte keeps its source offset):
//!
//! 1. the per-file `@jsxImportSource @verter/svelte-jsx` pragma — overriding
//!    the provider's project-level `jsxImportSource: "vue"` for THIS file only
//!    (D-ae(a)), even under `jsx: "preserve"`;
//! 2. the COMPLETE audited Svelte 5 rune surface as ambient `declare`s
//!    (`$props`/`$bindable`/`$state`/`$derived`/`$effect`/`$inspect`/`$host`,
//!    every namespace member, and `import type { Snippet } from "svelte"`) —
//!    rune CALL SITES stay verbatim, the prelude only TYPES them (D-u);
//! 3. the projection checkers/declarators (D-ae(c)): `__verter_attach` (the
//!    `{@attach}` target), the `__verter_snippet` brand declarator, and the
//!    `__verter_void` value checker (out-of-scope expressions route through it).
//!    `class` clsx forms (5.16) are typed by `SvelteHTMLElements`'
//!    `class?: ClassValue` in the intrinsic table — no separate class checker.
//!
//! The rune list is COMPLETE so no fixture using a namespace member fails the
//! clean-type-check gate spuriously. The declarations are ambient (`declare`)
//! so they introduce no runtime value and never collide with a user import.

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

/// Render the complete UNMAPPED prelude text for a JSX `namespace`.
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
    // One static block — the rune surface is fixed (the audited Svelte 5.56.x
    // surface), so a const string is exact and allocation-free to assemble.
    let pragma = namespace.pragma_line();
    let legacy = if legacy_mode {
        LEGACY_MAGIC_PRELUDE
    } else {
        ""
    };
    let mut out =
        String::with_capacity(pragma.len() + RUNE_AND_CHECKER_PRELUDE.len() + legacy.len());
    out.push_str(pragma);
    out.push_str(RUNE_AND_CHECKER_PRELUDE);
    out.push_str(legacy);
    out
}

/// The ambient rune surface + the three checkers/declarators.
///
/// Ambient (`declare`) so it introduces no runtime binding. `$props` etc. are
/// declared as functions with their namespace members attached via a merged
/// `declare namespace`. The Svelte 5 surface is COMPLETE per the D-u/D-ad
/// audit (5.56.x): `$state.raw`/`.snapshot`/`.eager`, `$derived.by`,
/// `$effect.pre`/`.tracking`/`.root`/`.pending`, `$inspect(...).with`/`.trace`,
/// `$props.id`, `$host`.
const RUNE_AND_CHECKER_PRELUDE: &str = r#"import type { Snippet } from "svelte";
import type { Attachment } from "svelte/attachments";
// --- Svelte 5 runes (ambient; call sites stay verbatim) ---
declare function $props<T = Record<string, unknown>>(): T;
declare namespace $props {
  function id(): string;
}
declare function $bindable<T = never>(fallback?: T): T;
declare function $state<T>(initial: T): T;
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
declare function $host<El extends HTMLElement = HTMLElement>(): El;
// --- Verter projection checkers/declarators (D-ae(c)) ---
declare function __verter_attach<E extends EventTarget>(attachment: Attachment<E>): void;
declare function __verter_snippet<Params extends unknown[]>(render: (...args: Params) => unknown): Snippet<Params>;
declare function __verter_void(...values: unknown[]): void;
// The host-element instance type for a projected tag (the `transition:`/`in:`/
// `out:`/`animate:` host node, D-ae). A known HTML/SVG tag resolves to its
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
// component bind it passes `InstanceType<typeof Child>["$props"][K]` (the typing
// is done in the PROJECTED TSX via TS — no Rust resolver call). A READONLY
// element binding routes to `__verter_bind_fn_read`, whose `get` is `null`-only
// (a readonly function binding must be the write-only `{null, set}` form).
declare function __verter_bind_fn<V>(get: (() => V) | null, set: (value: V) => void): void;
declare function __verter_bind_fn_read<V>(get: null, set: (value: V) => void): void;
// --- F8 `<svelte:component this={C}>` / `<svelte:self>` dynamic component.
// The `this` value must be a class-shaped component (an `abstract new (...) =>
// { $props }` constructor — the synth shape every `.svelte`/`.vue` component
// exposes). The props `P` are inferred DIRECTLY from the constructor's `$props`
// return member (NOT via `InstanceType<C>["$props"]`, which does not narrow
// over a generic `C` even at the call site — it stays `object`/`unknown`). The
// helper returns a FUNCTION COMPONENT typed by `P` (so `<__VerterDyn prop={x}>`
// checks `prop` against the component's own `$props`): a wrong prop FAILS, a
// non-component `this` FAILS the constructor constraint. `children` is
// permitted (svelte components accept slotted children) without forcing it onto
// the `$props` contract. The return type is `ReturnType<Snippet>` (the
// projected-element shape), never the pragma-bound `JSX.Element` (which is not
// in lexical scope in the prelude).
declare function __verter_dynamic_component<P>(
  component: abstract new (...args: never[]) => { $props: P },
): (props: P & { children?: unknown }) => ReturnType<Snippet>;
// --- F13 component `on:event={handler}` payload checking. A COMPONENT element's
// `on:select={h}` projects to `{...(__verter_event(Child, "select", h), {})}` —
// the helper extracts the component's `$events` map from its instance type and
// constrains the event NAME to `keyof $events` (an unknown event name FAILS) and
// the HANDLER to `$events[name]` (a wrong payload type FAILS). A component value
// is class-shaped (`new (...) => { $events }`); a component WITHOUT a `$events`
// member resolves to `{}` (no checkable events — every `on:` then FAILS the
// `keyof {}` constraint, which is the correct behaviour for an event-less
// component). This is the precise checked replacement for the rejected loose
// `on:`→`onclick` verbatim projection on component elements (a payload-checked
// event map, never an untyped event bag).
type __VerterEventsOf<C> =
  (C extends new (...args: any[]) => infer I ? I : never) extends { $events: infer E } ? E : {};
declare function __verter_event<C, K extends keyof __VerterEventsOf<C> & string>(
  component: C,
  name: K,
  handler: __VerterEventsOf<C>[K],
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
        // F8: the `<svelte:component>` / `<svelte:self>` dynamic-component
        // checker is declared with the class-shaped-constructor constraint,
        // inferring the props `P` DIRECTLY from the constructor's `{ $props: P }`
        // return member (NOT via `InstanceType<C>["$props"]` — see the prelude
        // comment for why that does not narrow over a generic `C`).
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(p.contains("declare function __verter_dynamic_component"));
        assert!(
            p.contains("abstract new (...args: never[]) => { $props: P }"),
            "the props are inferred directly from the constructor's $props member"
        );
        assert!(
            p.contains("(props: P & { children?: unknown })"),
            "the returned function component is typed by the inferred $props"
        );
    }

    #[test]
    fn prelude_declares_the_component_event_helper() {
        // F13: the component `on:event` payload checker is declared. It extracts
        // the component's `$events` map and constrains the event name to
        // `keyof __VerterEventsOf<C>` and the handler to its indexed payload type.
        let p = render_prelude(SvelteJsxNamespace::Html, true);
        assert!(p.contains("type __VerterEventsOf<C>"));
        assert!(p.contains(
            "declare function __verter_event<C, K extends keyof __VerterEventsOf<C> & string>"
        ));
        assert!(
            p.contains("handler: __VerterEventsOf<C>[K]"),
            "the handler is typed by the indexed event payload"
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
