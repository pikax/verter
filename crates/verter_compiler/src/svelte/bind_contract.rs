//! The CLOSED Svelte 5 `bind:` contract table — the SHARED SOURCE OF TRUTH for the
//! wide binding family, consumed by BOTH the IDE projection and the runtime client
//! codegen.
//!
//! Every Svelte-documented element binding name is pinned here (via the
//! generated [`SVELTE_BIND_CONTRACTS`] table) with its value TYPE (IDE-only), its
//! DIRECTION (read / read-write), and the RUNTIME emission metadata (which
//! `$.bind_*` helper / `bind_property` form, the call ARITY, the `bind_property`
//! EVENT name, the prelude CLEANUP, the `should_proxy` policy, and the host/tag).
//! The Svelte IDE projector consults the value type + direction to emit a
//! type-checked assignment-compatibility check in the projected `.svelte.tsx`; the
//! runtime client emitter consults the runtime metadata to emit the DATA-DRIVEN
//! `$.bind_*` call. One authored registry is the authority for both.
//!
//! This module lives OUTSIDE `svelte/ide/` so the runtime backend can depend on it
//! WITHOUT depending on the IDE projection (the Shared Optimized Codebase rule — one
//! source of truth, no duplicate fork).
//!
//! The table is GENERATED (`scripts/generate-svelte-bind-contract.mjs`) from a
//! closed authored registry and byte-pinned by
//! `crates/verter_compiler/tests/svelte_bind_contract_freshness.rs`, so a
//! registry change without a regen — or a hand-edit of the generated data —
//! fails the gate. The whole-table destructure test below (no `..`) forces a
//! conscious decision on every added binding.

use super::bind_contract_data::SVELTE_BIND_CONTRACTS;

/// The binding direction, from the bound LOCAL's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindDirection {
    /// Read-write: Svelte reads the local to set the DOM AND writes DOM changes
    /// back into the local — the local is INVARIANT with the value type.
    ReadWrite,
    /// Read-direction (readonly DOM property, DOM → local only): the local
    /// RECEIVES the value from the DOM and can never write back — the value type
    /// must be assignable to the local, and a userland write to the binding
    /// target is rejected.
    Read,
}

/// A binding that routes to a DEDICATED checker rather than the generic
/// value-type assignment-compat check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindSpecial {
    /// `bind:this` — host-instance assignment-compat (the projector substitutes
    /// the element's host-instance type and routes to the `this` checker).
    This,
    /// `bind:group` — checkbox-vs-radio shared selection (the projector inspects
    /// the sibling `type` attribute and routes to the radio/checkbox checker).
    Group,
    /// No special routing — the generic value-type check applies.
    None,
}

/// The HOST-SCOPE axis of a bind row — which special-element hosts a bind is valid on,
/// ORTHOGONAL to the regular-element `tags` constraint. Mirrors svelte's
/// `binding_properties` `valid_elements` / `invalid_elements` for the four special hosts
/// (`<svelte:window>`, `<svelte:document>`, `<svelte:body>`, `<svelte:element>`), so the
/// host gate consults a proven fact per bind instead of a name heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindHostScope {
    /// Valid on a regular DOM element (per the `tags` constraint) PLUS `<svelte:body>` and
    /// `<svelte:element>` when the bind admits any element (`tags == "*"` /
    /// `"contenteditable"`), but NOT on the window/document hosts. The DEFAULT. Mirrors
    /// svelte's rows with no special `valid_elements` (the dimension / contenteditable rows
    /// additionally carry `invalid_elements: [svelte:window, svelte:document]`).
    Element,
    /// Valid ONLY on `<svelte:window>` (svelte `valid_elements: ['svelte:window']`) — the
    /// `innerWidth` / `scrollX` / `online` / `devicePixelRatio` family. The `tags` column is
    /// documentary (the bind never resolves on a regular tag).
    Window,
    /// Valid ONLY on `<svelte:document>` (svelte `valid_elements: ['svelte:document']`) — the
    /// `activeElement` / `fullscreenElement` / `pointerLockElement` / `visibilityState`
    /// family.
    Document,
    /// Valid on EVERY host: regular elements (per `tags`) AND all four special hosts. Mirrors
    /// svelte's rows with neither `valid_elements` nor `invalid_elements` — only `focused`
    /// and `this`.
    Universal,
}

/// The ACCEPTED TARGET-EXPRESSION shape policy for a bind name — which expression
/// forms official `svelte@5.56.3` admits as the bound target.
///
/// Most binds accept BOTH an lvalue (an `Identifier` / `MemberExpression`) AND a
/// two-element function-pair `{get, set}` (a `SequenceExpression`). `bind:group` is
/// the SOLE exception: official's `BindDirective` analysis throws
/// `bind_group_invalid_expression` for ANY `SequenceExpression` target (the
/// getter/setter pair form is meaningless for the shared checkbox/radio selection
/// group), BEFORE the two-element shape check. A STATIC, data-driven column (NOT a
/// `name == "group"` hard-code at the call site) so the official-reject gate consults
/// a proven policy fact per bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindTargetPolicy {
    /// The default: an `Identifier` / `MemberExpression` lvalue OR a two-element
    /// function-pair `{get, set}` is accepted (`bind:value` / `bind:checked` / media /
    /// dimension / contenteditable / property binds).
    LvalueOrFunctionPair,
    /// Only an `Identifier` / `MemberExpression` lvalue is accepted; ANY
    /// `SequenceExpression` target (the function-pair form, regardless of element
    /// count) is an official reject carrying [`official_code`](Self::IdentifierOrMemberOnly::official_code).
    /// Currently only `bind:group`.
    IdentifierOrMemberOnly {
        /// The EXACT official `svelte@5.56.3` diagnostic code a `SequenceExpression`
        /// target on this bind mirrors (`bind_group_invalid_expression`).
        official_code: &'static str,
    },
}

/// The EMITTABLE runtime helper — which `$.bind_*` client helper the native client
/// backend emits (or the generic `$.bind_property` form). A STATIC enum (not a hot
/// tag-string splitter) so the runtime emit path is a DATA-DRIVEN match over a proven
/// helper fact, never a name-string dispatch. Each variant maps to exactly one pinned
/// `svelte@5.56.3` client helper shape.
///
/// This is the CLOSED set of helpers the native client runtime can emit. It is carried
/// on [`RuntimeBindRouting`] (the routing the emitter consumes), so a routing can ONLY
/// name an emittable helper by construction — there is no "unsupported" helper variant.
/// The full OFFICIAL helper identity for a bind row (including helpers the runtime does
/// not yet emit) is the separate [`OfficialRuntimeHelper`] fact on the contract row; the
/// orthogonal support status is [`RuntimeSupport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeHelper {
    /// `$.bind_value(el, get, set)` — `<input>`/`<textarea>` `bind:value`.
    Value,
    /// `$.bind_select_value(el, get, set)` — `<select>` `bind:value` (single +
    /// `multiple`, the same helper).
    SelectValue,
    /// `$.bind_checked(el, get, set)` — `<input type="checkbox">` `bind:checked`.
    Checked,
    /// `$.bind_group(binding_group, [], el, get, set)` — `bind:group` (the
    /// component-FUNCTION-scoped `binding_group`).
    Group,
    /// `$.bind_current_time(el, get, set)` — media `bind:currentTime`.
    CurrentTime,
    /// `$.bind_paused(el, get, set)` — media `bind:paused`.
    Paused,
    /// `$.bind_played(el, set)` — media `bind:played` (SETTER-ONLY).
    Played,
    /// `$.bind_element_size(el, 'name', set)` — dimension bindings (`clientWidth` /
    /// `clientHeight` / `offsetWidth` / `offsetHeight`), SETTER-ONLY, the dimension
    /// name passed as a string literal arg.
    ElementSize,
    /// `$.bind_content_editable('name', el, get, set)` — contenteditable bindings
    /// (`innerHTML` / `innerText` / `textContent`), the property name as a string
    /// literal arg.
    ContentEditable,
    /// `$.bind_property('name', 'event', el, set [, get])` — the generic DOM-property
    /// bind (`bind:open` → `('open','toggle',…)`, media `bind:duration` →
    /// `('duration','durationchange',…)` read-only). The property + event names are
    /// supplied as string-literal args; the getter is present iff [`BindDirection`] is
    /// `ReadWrite`.
    Property,
    /// `$.bind_this(host, set, get)` — `bind:this`. (Routing is host-specific; the
    /// element host is owned by 5c, the component / special-element hosts by 5f.)
    This,
    /// `$.bind_window_size('<name>', set)` — `<svelte:window>` dimension reads
    /// (`innerWidth` / `innerHeight` / `outerWidth` / `outerHeight`). The dimension NAME is
    /// the first string-literal arg, NO host expr, setter-only.
    WindowSize,
    /// `$.bind_window_scroll('x'|'y', get, set)` — `<svelte:window>` scroll positions
    /// (`scrollX` / `scrollY`). The runtime axis name is REMAPPED (`'x'` / `'y'`); the helper
    /// is READ-WRITE (get+set), unlike the set-only `WindowSize`.
    WindowScroll,
    /// `$.bind_online(set)` — `<svelte:window bind:online>` (setter-only, NO name, NO host).
    Online,
    /// `$.bind_focused(host, set)` — `bind:focused` (host expr + setter-only). The host is
    /// the element var on a regular element and `$.window` on the window host.
    Focused,
    /// `$.bind_active_element(set)` — `<svelte:document bind:activeElement>` (the DEDICATED
    /// setter-only helper, NO name, NO host expr — NOT the generic `$.bind_property`).
    ActiveElement,
}

/// The OFFICIAL `svelte@5.56.3` runtime helper IDENTITY for a bind-CONTRACT ROW — the
/// machine-readable fact of which `$.bind_*` / `bind_property` form the official compiler
/// emits, INDEPENDENT of whether the native client runtime currently emits it (that is the
/// orthogonal [`RuntimeSupport`] axis). Splitting helper IDENTITY from support STATUS keeps
/// the closed contract a faithful fact table: a row the runtime does not yet emit records
/// its REAL official helper (never an erased sentinel), so the implementation that later
/// supports it has the pinned shape on hand. Every variant is oracle-verified against the
/// pinned compiler. The emittable subset maps to a [`RuntimeHelper`] via
/// [`Self::emittable_runtime_helper`].
///
/// This is the CONTRACT-ROW helper vocabulary. The builtin form-control binds (`value` /
/// `checked`), which are DELIBERATELY ABSENT from [`SVELTE_BIND_CONTRACTS`] (they ride the
/// plain JSX intrinsic-attribute path for the IDE), are NOT rows — they have no
/// `official_helper` field; their emittable helper (`$.bind_value` / `$.bind_select_value`
/// / `$.bind_checked`) is built directly as a [`RuntimeHelper`] in the builtin arm of
/// [`resolve_runtime_bind`]. So [`RuntimeHelper`] (the full emittable set: rows + builtins)
/// carries `Value` / `SelectValue` / `Checked`, while this row-scoped enum does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficialRuntimeHelper {
    /// `$.bind_group` — `bind:group`.
    Group,
    /// `$.bind_current_time` — media `bind:currentTime`.
    CurrentTime,
    /// `$.bind_paused` — media `bind:paused`.
    Paused,
    /// `$.bind_played` — media `bind:played` (setter-only).
    Played,
    /// `$.bind_element_size` — element dimension binds (`clientWidth`/…), setter-only.
    ElementSize,
    /// `$.bind_content_editable` — contenteditable binds (`innerHTML`/…).
    ContentEditable,
    /// `$.bind_property('name', 'event', …)` — the generic DOM-property bind. The
    /// SUPPORTED `bind:open` (details) + readonly media `bind:duration`, AND the
    /// runtime-unsupported `indeterminate` / `naturalWidth` / `naturalHeight` /
    /// `videoWidth` / `videoHeight` rows (whose official helper IS this generic form).
    Property,
    /// `$.bind_this` — `bind:this` (host-routed).
    This,
    /// `$.bind_files(input, get, set)` — `<input type="file">` `bind:files` (get/set,
    /// read-write). Runtime-unsupported in 5c.
    Files,
    /// `$.bind_focused(el, set)` — `bind:focused` (setter-only). Runtime-supported (5f-b).
    Focused,
    /// `$.bind_volume(el, get, set)` — media `bind:volume` (get/set). Runtime-unsupported.
    Volume,
    /// `$.bind_muted(el, get, set)` — media `bind:muted` (get/set). Runtime-unsupported.
    Muted,
    /// `$.bind_playback_rate(el, get, set)` — media `bind:playbackRate` (get/set).
    /// Runtime-unsupported.
    PlaybackRate,
    /// `$.bind_buffered(el, set)` — readonly media `bind:buffered` (setter-only).
    /// Runtime-unsupported.
    Buffered,
    /// `$.bind_seekable(el, set)` — readonly media `bind:seekable` (setter-only).
    /// Runtime-unsupported.
    Seekable,
    /// `$.bind_seeking(el, set)` — readonly media `bind:seeking` (setter-only).
    /// Runtime-unsupported.
    Seeking,
    /// `$.bind_ended(el, set)` — readonly media `bind:ended` (setter-only).
    /// Runtime-unsupported.
    Ended,
    /// `$.bind_ready_state(el, set)` — readonly media `bind:readyState` (setter-only).
    /// Runtime-unsupported.
    ReadyState,
    /// `$.bind_resize_observer(el, 'name', set)` — resize-observer binds
    /// (`contentRect`/`contentBoxSize`/`borderBoxSize`/`devicePixelContentBoxSize`),
    /// setter-only with a string-literal name arg. Runtime-unsupported.
    ResizeObserver,
    /// `$.bind_window_size('<name>', set)` — `<svelte:window>` dimension reads (5f-b).
    WindowSize,
    /// `$.bind_window_scroll('x'|'y', get, set)` — `<svelte:window>` scroll positions (5f-b).
    WindowScroll,
    /// `$.bind_online(set)` — `<svelte:window bind:online>` (5f-b).
    Online,
    /// `$.bind_active_element(set)` — `<svelte:document bind:activeElement>` (5f-b).
    ActiveElement,
}

impl OfficialRuntimeHelper {
    /// The EMITTABLE [`RuntimeHelper`] this official helper maps to when the native client
    /// runtime supports the bind, or `None` for an official helper the runtime does not
    /// emit (the dedicated `bind_files` / `bind_focused` / `bind_volume` / `bind_muted` /
    /// `bind_playback_rate` / media-readiness / resize-observer helpers). Support is the
    /// [`RuntimeSupport`] axis, NOT this mapping — a `Supported` non-`this` row's official
    /// helper always maps to `Some` (pinned by the contract self-tests), and an
    /// `Unsupported` row may carry either an emittable-OR-non-emittable official helper.
    #[must_use]
    pub fn emittable_runtime_helper(self) -> Option<RuntimeHelper> {
        Some(match self {
            Self::Group => RuntimeHelper::Group,
            Self::CurrentTime => RuntimeHelper::CurrentTime,
            Self::Paused => RuntimeHelper::Paused,
            Self::Played => RuntimeHelper::Played,
            Self::ElementSize => RuntimeHelper::ElementSize,
            Self::ContentEditable => RuntimeHelper::ContentEditable,
            Self::Property => RuntimeHelper::Property,
            Self::This => RuntimeHelper::This,
            // The dedicated special-host helpers (5f-b) — the native client runtime EMITS
            // them (their rows are `RuntimeSupport::Supported`).
            Self::Focused => RuntimeHelper::Focused,
            Self::WindowSize => RuntimeHelper::WindowSize,
            Self::WindowScroll => RuntimeHelper::WindowScroll,
            Self::Online => RuntimeHelper::Online,
            Self::ActiveElement => RuntimeHelper::ActiveElement,
            // The dedicated media/file helpers + the resize-observer family are NOT emitted
            // by the native client runtime yet — these rows fail closed at
            // `resolve_runtime_bind` (via their `RuntimeSupport::Unsupported`), so the
            // mapping is `None`.
            Self::Files
            | Self::Volume
            | Self::Muted
            | Self::PlaybackRate
            | Self::Buffered
            | Self::Seekable
            | Self::Seeking
            | Self::Ended
            | Self::ReadyState
            | Self::ResizeObserver => return None,
        })
    }
}

/// Whether the native client runtime currently EMITS a bind-contract row — orthogonal to
/// the official helper IDENTITY ([`OfficialRuntimeHelper`]). The IDE projection consumes
/// every row's value-type / direction columns regardless of support; only the RUNTIME
/// router ([`resolve_runtime_bind`]) refuses an `Unsupported` row (fail-closed). A row's
/// support flips to `Supported` when the runtime gains the helper plumbing + goldens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSupport {
    /// The native client runtime emits this bind (its [`OfficialRuntimeHelper`] maps to an
    /// emittable [`RuntimeHelper`]).
    Supported,
    /// The native client runtime does NOT emit this bind yet. The IDE row is still real
    /// (the projector type-checks the bind via the value-type / direction columns); only
    /// [`resolve_runtime_bind`] fails it closed. The official helper identity is PRESERVED
    /// on the row — absent-row fail-closed and helper-identity erasure are both unacceptable.
    Unsupported,
}

/// The getter/setter ARITY of a runtime bind helper call — whether the emitter passes
/// a get/set PAIR, a SETTER-ONLY thunk, or the generic `bind_property` form (whose
/// getter presence is decided by direction). A STATIC enum so the emitter never
/// guesses arity from the helper name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperArity {
    /// `(… , () => GET, ($$value) => SET)` — a get/set closure PAIR.
    GetSet,
    /// `(… , ($$value) => SET)` — a SETTER-ONLY thunk (read-only DOM property; the
    /// local only RECEIVES the value).
    SetOnly,
    /// The `$.bind_property(prop, event, el, set [, get])` form — `set` always
    /// present; `get` present iff [`BindDirection::ReadWrite`].
    Property,
}

/// The prelude CLEANUP a host node emits BEFORE its bind call — the official
/// per-host default-clearing statement. A STATIC enum (not a name check).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindPrelude {
    /// No prelude.
    None,
    /// `$.remove_input_defaults(el);` — emitted before a `bind:checked` /
    /// `bind:group` on an `<input>`.
    RemoveInputDefaults,
    /// `$.remove_textarea_child(el);` — emitted before a `bind:value` on a
    /// `<textarea>`.
    RemoveTextareaChild,
}

/// One row of the closed bind-contract table. Carries the IDE columns (`value_type`,
/// `direction`, `tags`, `special`) AND the runtime emission columns (`helper`,
/// `arity`, `prop_event`, `prelude`, `should_proxy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindContract {
    /// The binding local (`value` in `bind:value`).
    pub name: &'static str,
    /// The binding direction.
    pub direction: BindDirection,
    /// The TS type of the bound value (a `svelte`/DOM type expression). For
    /// host-instance bindings (`bind:this`) the literal `{HOST}` placeholder is
    /// substituted by the projector with the element's host-instance type. IDE-only.
    pub value_type: &'static str,
    /// The applicable lowercase tag set (comma-separated), `*` for any element,
    /// or `contenteditable` (documentary — any element carrying the attribute).
    pub tags: &'static str,
    /// The special-host scope (window / document / element / body applicability),
    /// ORTHOGONAL to `tags`. Decides whether the bind resolves on a special host so a
    /// host-scoped bind (`scrollX`) is window-only and the wrong-host pairs fail closed.
    pub host_scope: BindHostScope,
    /// The dedicated-checker routing marker, if any. IDE column.
    pub special: BindSpecial,
    /// The accepted target-expression shape policy — whether a function-pair
    /// `{get, set}` (`SequenceExpression`) target is admitted, or rejected with an
    /// official code (only `bind:group`). Consumed by the official-reject gate.
    pub target_policy: BindTargetPolicy,
    /// The OFFICIAL `svelte@5.56.3` runtime helper IDENTITY — which `$.bind_*` /
    /// `bind_property` form the official compiler emits, preserved even for rows the native
    /// runtime does not yet emit. Contract/runtime column (the orthogonal support status is
    /// [`Self::support`]).
    pub official_helper: OfficialRuntimeHelper,
    /// Whether the native client runtime currently emits this bind. Runtime column —
    /// [`resolve_runtime_bind`] refuses an `Unsupported` row (the IDE row stays real).
    pub support: RuntimeSupport,
    /// The runtime getter/setter arity. Runtime column.
    pub arity: HelperArity,
    /// The `$.bind_property` EVENT name (`durationchange` / `toggle` / …) for a
    /// [`RuntimeHelper::Property`] row, else the empty string. Runtime column.
    pub prop_event: &'static str,
    /// The prelude cleanup the host emits before this bind. Runtime column.
    pub prelude: BindPrelude,
    /// The `should_proxy` policy — whether the setter takes the third `true`
    /// proxy-flag argument (`$.set(local, $$value, true)`). FALSE for every ordinary
    /// DOM bind. Runtime column.
    ///
    /// DOCUMENTARY (latent-trap warning): this contract-row field is consumed ONLY by
    /// the bind-contract test (`bind_contract_tests.rs`) — it is NOT wired into setter
    /// generation. The EMITTED proxy flag is HOST-DRIVEN at projection time by
    /// `bind_target_is_special_host` (`client_plan_bind.rs`), NOT by this row: e.g. the
    /// `focused` row carries `should_proxy: false` yet a `<svelte:window bind:focused>`
    /// correctly emits `$.set(f, $$value, true)` because the WINDOW HOST drives the
    /// proxy. Do NOT wire `routing.should_proxy` into the setter to "simplify" — that
    /// would silently REGRESS the host-driven `<svelte:window>` binds (a `false` row
    /// would drop the required `true` flag). Keep the emitted proxy host-driven.
    pub should_proxy: bool,
}

impl BindContract {
    /// Whether this contract's `tags` constraint admits the given lowercase REGULAR-element
    /// tag. `*` and `contenteditable` admit any element (the projector does not enforce the
    /// contenteditable attribute presence — a userland mismatch is a rare authoring error,
    /// and the value type still checks). Does NOT consult `host_scope` — callers that resolve
    /// against a possible special host use [`Self::applies_to_host`].
    #[must_use]
    pub fn applies_to_tag(&self, tag: &str) -> bool {
        match self.tags {
            "*" | "contenteditable" => true,
            list => list.split(',').any(|t| t == tag),
        }
    }

    /// Whether the bind applies on `host` — a REGULAR-element tag (`div` / `input` / …) OR a
    /// special-host token (`svelte:window` / `svelte:document` / `svelte:body` /
    /// `svelte:element`). Combines the `host_scope` axis with the `tags` constraint, faithful
    /// to svelte's `valid_elements` / `invalid_elements`:
    ///
    /// - `svelte:window` ⇒ `host_scope` is `Window` or `Universal`;
    /// - `svelte:document` ⇒ `host_scope` is `Document` or `Universal`;
    /// - `svelte:body` / `svelte:element` (a generic element host that is neither a concrete
    ///   tag nor a global) ⇒ `Universal`, OR `Element` when the bind admits any element
    ///   (`tags == "*"` / `"contenteditable"`) — so `bind:clientWidth` is valid on a body /
    ///   dynamic element but `bind:value` (input/textarea/select) is not;
    /// - a concrete regular tag ⇒ `host_scope` is `Element` or `Universal` AND
    ///   [`applies_to_tag`](Self::applies_to_tag) admits it (so a window-only bind never
    ///   resolves on a regular element).
    #[must_use]
    pub fn applies_to_host(&self, host: &str) -> bool {
        match host {
            "svelte:window" => matches!(
                self.host_scope,
                BindHostScope::Window | BindHostScope::Universal
            ),
            "svelte:document" => {
                matches!(
                    self.host_scope,
                    BindHostScope::Document | BindHostScope::Universal
                )
            }
            "svelte:body" | "svelte:element" => {
                matches!(self.host_scope, BindHostScope::Universal)
                    || (matches!(self.host_scope, BindHostScope::Element)
                        && matches!(self.tags, "*" | "contenteditable"))
            }
            tag => {
                matches!(
                    self.host_scope,
                    BindHostScope::Element | BindHostScope::Universal
                ) && self.applies_to_tag(tag)
            }
        }
    }
}

/// Look up the bind contract for `name` that applies to the lowercase `tag`.
///
/// A name may appear once in the table with a tag constraint; the lookup
/// returns the row only when the tag is admitted. A name absent from the table
/// (or present but not admitted for `tag`) returns `None` — the projector then
/// treats it as an unknown binding (no F4 contract). The bind names that resolve
/// through the plain JSX intrinsic table (`value`, `checked`) and through
/// `SvelteHTMLElements` attributes (`defaultValue`, `defaultChecked`) are
/// DELIBERATELY ABSENT here — they are not wide-family contracts.
#[must_use]
pub fn lookup_bind_contract(name: &str, tag: &str) -> Option<&'static BindContract> {
    SVELTE_BIND_CONTRACTS
        .iter()
        .find(|c| c.name == name && c.applies_to_host(tag))
}

/// The accepted target-expression shape policy for `bind:<name>` on the lowercase
/// `tag` — the data-driven authority the official-reject gate consults to decide
/// whether a function-pair (`SequenceExpression`) target is admitted.
///
/// A bind with a contract row carries its row's policy; the builtin form-control
/// binds (`value` / `checked`), which are DELIBERATELY ABSENT from the contract table
/// (they ride the plain JSX intrinsic path) AND any `(name, tag)` with no admitted
/// row, default to [`BindTargetPolicy::LvalueOrFunctionPair`] — they accept the
/// function-pair form, matching official. Only `bind:group` (on its `<input>` host)
/// resolves to [`BindTargetPolicy::IdentifierOrMemberOnly`].
#[must_use]
pub fn bind_target_policy(name: &str, tag: &str) -> BindTargetPolicy {
    lookup_bind_contract(name, tag)
        .map(|c| c.target_policy)
        .unwrap_or(BindTargetPolicy::LvalueOrFunctionPair)
}

/// The RUNTIME emission routing for one `bind:` on a DOM host — the single typed
/// fact the native client backend's classifier + plan + emitter consume to emit the
/// correct `$.bind_*` / `bind_property` call. A STATIC descriptor (no source text, no
/// helper-name strings at the emit hot path).
///
/// This is the runtime analogue of [`lookup_bind_contract`]: it covers BOTH the
/// builtin form-control binds (`value` / `checked`) — which are DELIBERATELY ABSENT
/// from the IDE contract table (they ride the plain JSX intrinsic-attribute path for
/// the IDE) but ARE real runtime binds with dedicated helpers — AND the wide
/// `bind:` family (every [`SVELTE_BIND_CONTRACTS`] row), so the runtime has ONE
/// routing authority. The element host (`textarea` vs `select` vs `input`) selects the
/// `value` helper, matching the pinned `svelte@5.56.3` shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeBindRouting {
    /// The `$.bind_*` helper / `bind_property` form to emit.
    pub helper: RuntimeHelper,
    /// The getter/setter arity of the helper call.
    pub arity: HelperArity,
    /// The bind direction (decides the `bind_property` getter presence).
    pub direction: BindDirection,
    /// The `$.bind_property` event name (only for the property form), else `""`.
    pub prop_event: &'static str,
    /// The prelude cleanup the host emits before the bind call.
    pub prelude: BindPrelude,
    /// The should_proxy policy — whether the setter takes the 3rd `true` arg. FALSE
    /// for every DOM bind this function returns (the proxy flag is a 5f
    /// component/window-host policy).
    pub should_proxy: bool,
}

/// Resolve the runtime emission routing for a `bind:<name>` on the lowercase DOM
/// host `tag`, or `None` for a `(name, tag)` pair the native DOM client backend does
/// NOT emit (the caller then fails closed).
///
/// The builtin form-control binds are routed first (they are not in the IDE
/// contract table): `value` selects `$.bind_value` on `<input>`/`<textarea>` (the
/// textarea adds the `remove_textarea_child` prelude) and `$.bind_select_value` on
/// `<select>`; `checked` selects `$.bind_checked` on `<input>` (with the
/// `remove_input_defaults` prelude). Every other name delegates to the shared
/// contract row's runtime columns. `this` is host-routed (element host owned by 5c,
/// component host by 5f) and intentionally returns `None` here — the runtime handles
/// `bind:this` through its own element-host path, not this DOM-value router.
#[must_use]
pub fn resolve_runtime_bind(name: &str, tag: &str) -> Option<RuntimeBindRouting> {
    // (1) The builtin form-control binds — NOT in the IDE contract table.
    match (name, tag) {
        ("value", "input") | ("value", "textarea") => {
            return Some(RuntimeBindRouting {
                helper: RuntimeHelper::Value,
                arity: HelperArity::GetSet,
                direction: BindDirection::ReadWrite,
                prop_event: "",
                // `<input bind:value>` clears its form defaults via
                // `$.remove_input_defaults` (the pinned `bind_value_and_this` /
                // `hello_input` goldens); `<textarea bind:value>` strips its child
                // content via `$.remove_textarea_child` (oracle CASE `textarea_value`).
                prelude: if tag == "textarea" {
                    BindPrelude::RemoveTextareaChild
                } else {
                    BindPrelude::RemoveInputDefaults
                },
                should_proxy: false,
            });
        }
        ("value", "select") => {
            return Some(RuntimeBindRouting {
                helper: RuntimeHelper::SelectValue,
                arity: HelperArity::GetSet,
                direction: BindDirection::ReadWrite,
                prop_event: "",
                prelude: BindPrelude::None,
                should_proxy: false,
            });
        }
        ("checked", "input") => {
            return Some(RuntimeBindRouting {
                helper: RuntimeHelper::Checked,
                arity: HelperArity::GetSet,
                direction: BindDirection::ReadWrite,
                prop_event: "",
                prelude: BindPrelude::RemoveInputDefaults,
                should_proxy: false,
            });
        }
        _ => {}
    }
    // (2) The wide `bind:` family — delegate to the shared contract row. `this` is
    // host-routed by the runtime element-host path, not this DOM-value router.
    let row = lookup_bind_contract(name, tag)?;
    if row.special == BindSpecial::This {
        return None;
    }
    // An UNSUPPORTED row fails closed: the native client runtime does not emit it (the
    // IDE `lookup_bind_contract` STILL returns the row so the projector type-checks the
    // bind). Refusal rides the SUPPORT status, NEVER the helper identity: rows like
    // `indeterminate` / `naturalWidth` carry the real generic-`Property` official helper
    // yet stay unsupported, so a helper-identity check would wrongly emit them. The
    // official helper (incl. the dedicated `$.bind_files` / `$.bind_focused` / … and the
    // resize-observer helpers) is PRESERVED on the row for the block that later supports it.
    if row.support == RuntimeSupport::Unsupported {
        return None;
    }
    // A `Supported`, non-`this` row's official helper is an emittable runtime helper by
    // construction (pinned by `supported_rows_official_helper_maps_to_an_emittable_runtime_helper`).
    let helper = row
        .official_helper
        .emittable_runtime_helper()
        .expect("a Supported non-this bind-contract row must map to an emittable runtime helper");
    // The `group` row needs the `remove_input_defaults` prelude on its `<input>`
    // host (the contract carries it).
    Some(RuntimeBindRouting {
        helper,
        arity: row.arity,
        direction: row.direction,
        prop_event: row.prop_event,
        prelude: row.prelude,
        should_proxy: row.should_proxy,
    })
}
